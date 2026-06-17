/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! End-to-end test for the wire-format `__type` discriminator lift.
//!
//! Exercises the full pipeline:
//!
//! ```text
//! JSON wire bytes
//!     ↓ JsonCodec::create_deserializer
//! JsonDeserializer<'a>
//!     ↓ read_discriminated_document (lifts top-level __type)
//! DiscriminatedDocument  (wraps a fully-owned aws_smithy_types::Document
//!                         and carries the lifted FQN as Option<String>)
//!     ↓ TypeRegistry::deserialize_document
//! TypeErasedBox
//!     ↓ downcast::<ConcreteShape>()
//! Typed shape with field values asserted
//! ```
//!
//! This is the user-visible payoff of:
//!  * the `__type` lift in `aws-smithy-json::codec::deserializer`, and
//!  * the cross-lifetime registry-lookup APIs on `TypeRegistry`.

use aws_sdk_dynamodb::types::Capacity;
use aws_sdk_dynamodb::Client;
use aws_smithy_json::codec::JsonCodec;
use aws_smithy_schema::codec::Codec;

#[test]
fn wire_bytes_with_absolute_type_round_trip_through_registry() {
    // Wire bytes shaped like a real-world JSON document of a Capacity
    // shape, prefixed with `__type` carrying an absolute Smithy shape
    // ID. This is the form a service emits when it includes typed
    // documents in a response.
    //
    // Members are keyed by the Smithy member name (the schema
    // generator preserves these as the `member_name` field), not the
    // snake_case Rust field name.
    let bytes = br#"{"__type":"com.amazonaws.dynamodb#Capacity","ReadCapacityUnits":1.5,"WriteCapacityUnits":2.5,"CapacityUnits":3.0}"#;

    let codec = JsonCodec::default();
    let mut deser = codec.create_deserializer(bytes);
    let doc = deser
        .read_discriminated_document()
        .expect("well-formed JSON parses successfully");

    // Step 1: the deserializer lifted the top-level __type into the
    // discriminator slot of the DiscriminatedDocument wrapper. The
    // unified design carries the discriminator as an owned FQN
    // string rather than a structured ShapeId — namespace and
    // shape-name parsing is the caller's responsibility if needed.
    let fqn = doc
        .discriminator()
        .expect("absolute __type lifted into discriminator");
    assert_eq!(fqn, "com.amazonaws.dynamodb#Capacity");

    // Step 2: __type was dropped from the resulting map; only the
    // body members remain. The data lives on the inner Document
    // (DiscriminatedDocument is a thin wrapper around it).
    let map = doc
        .document()
        .as_object()
        .expect("top-level value is an object");
    assert!(
        !map.contains_key("__type"),
        "lift must drop __type from the resulting map",
    );
    assert_eq!(map.len(), 3);

    // Step 3: the registry looks up the discriminator FQN against
    // its `'static`-keyed table, finds Capacity, and dispatches to
    // the generated deserialize fn.
    let typed = Client::registry()
        .deserialize_document(&doc)
        .expect("Capacity is registered and the document is well-formed");

    // Step 4: downcast to recover the concrete type.
    let capacity = typed
        .downcast::<Capacity>()
        .expect("registered DeserializeFn returns the correct concrete type");

    assert_eq!(capacity.read_capacity_units, Some(1.5));
    assert_eq!(capacity.write_capacity_units, Some(2.5));
    assert_eq!(capacity.capacity_units, Some(3.0));
}

#[test]
fn wire_bytes_with_relative_type_does_not_lift() {
    // Relative `__type` (no `#`) is not currently lifted — relative
    // resolution against `JsonCodecSettings::default_namespace` is
    // a follow-up. The string remains in the result map and the
    // discriminator is None.
    let bytes = br#"{"__type":"Capacity","ReadCapacityUnits":1.5}"#;

    let codec = JsonCodec::default();
    let mut deser = codec.create_deserializer(bytes);
    let doc = deser
        .read_discriminated_document()
        .expect("well-formed JSON parses successfully");

    assert!(
        doc.discriminator().is_none(),
        "relative __type values must not be lifted (no default-namespace resolution yet)",
    );
    let map = doc
        .document()
        .as_object()
        .expect("top-level value is an object");
    assert_eq!(
        map.get("__type").and_then(|d| d.as_string()),
        Some("Capacity"),
    );
}

#[test]
fn wire_bytes_with_unknown_absolute_type_errors_at_registry() {
    // `__type` is well-formed and gets lifted, but the discriminator
    // doesn't match any registered shape. The registry returns the
    // matching `SerdeError::UnknownMember`.
    let bytes = br#"{"__type":"com.example#NotARealShape"}"#;

    let codec = JsonCodec::default();
    let mut deser = codec.create_deserializer(bytes);
    let doc = deser
        .read_discriminated_document()
        .expect("well-formed JSON parses successfully");

    let fqn = doc.discriminator().expect("absolute lift succeeded");
    assert_eq!(fqn, "com.example#NotARealShape");

    let result = Client::registry().deserialize_document(&doc);
    assert!(
        result.is_err(),
        "unregistered shape id must cause registry lookup to fail",
    );
}
