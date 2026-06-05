/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! End-to-end test for `Client::registry()` (Phase 4 of the Document Types &
//! Type Registries SEP).
//!
//! Builds a `Document` with a discriminator pointing to a real DynamoDB shape
//! (`com.amazonaws.dynamodb#Capacity`), looks the entry up in the registry,
//! deserializes via `TypeRegistry::deserialize_document`, and downcasts to the
//! concrete type to assert field values round-tripped correctly.

use std::collections::HashMap;

use aws_sdk_dynamodb::types::Capacity;
use aws_sdk_dynamodb::Client;
use aws_smithy_schema::document::Document;
use aws_smithy_schema::shape_id;

#[test]
fn registry_deserialize_document_round_trip() {
    // Build a Document representing a Capacity shape:
    //   { "ReadCapacityUnits": 1.5, "WriteCapacityUnits": 2.5, "CapacityUnits": 3.0 }
    // The map keys must match the Smithy member names (the schema generator
    // uses the original Smithy names as `member_name`, not the snake_case Rust
    // field names).
    let mut members: HashMap<String, Document> = HashMap::new();
    members.insert("ReadCapacityUnits".to_owned(), Document::double(1.5));
    members.insert("WriteCapacityUnits".to_owned(), Document::double(2.5));
    members.insert("CapacityUnits".to_owned(), Document::double(3.0));
    let doc =
        Document::map(members).with_discriminator(shape_id!("com.amazonaws.dynamodb", "Capacity"));

    // Look up via the package-level registry and deserialize.
    let registry = Client::registry();
    let typed = registry
        .deserialize_document(&doc)
        .expect("Capacity is registered and the document is well-formed");

    // Downcast to the concrete type.
    let capacity = typed
        .downcast::<Capacity>()
        .expect("the registered DeserializeFn returns a Capacity");

    assert_eq!(capacity.read_capacity_units, Some(1.5));
    assert_eq!(capacity.write_capacity_units, Some(2.5));
    assert_eq!(capacity.capacity_units, Some(3.0));
}

#[test]
fn registry_returns_none_for_unregistered_shape() {
    // Unknown shape id → registry has no entry → deserialize_document errors.
    let doc =
        Document::map(HashMap::new()).with_discriminator(shape_id!("com.example", "NotARealShape"));

    let result = Client::registry().deserialize_document(&doc);
    assert!(result.is_err(), "unregistered shape should error");
}

#[test]
fn registry_errors_when_discriminator_missing() {
    // No discriminator → deserialize_document errors.
    let doc = Document::map(HashMap::new());

    let result = Client::registry().deserialize_document(&doc);
    assert!(
        result.is_err(),
        "documents without a discriminator should error"
    );
}

#[test]
fn registry_excludes_errors() {
    // Per the SEP, error shapes are not in the primary type registry — they
    // belong in the (forthcoming) `Client::error_registry()`. ResourceInUseException
    // is a real DynamoDB error shape; assert it's not in the primary registry.
    let id = shape_id!("com.amazonaws.dynamodb", "ResourceInUseException");
    assert!(
        Client::registry().schema_for(&id).is_none(),
        "error shapes must not appear in the primary type registry"
    );
}

#[test]
fn registry_excludes_synthetic_input_output() {
    // Operation input/output shapes have synthetic IDs. They're codegen
    // artifacts and intentionally excluded from the primary registry.
    let synthetic = shape_id!("com.amazonaws.dynamodb.synthetic", "GetItemInput");
    assert!(
        Client::registry().schema_for(&synthetic).is_none(),
        "synthetic operation input/output shapes must not appear in the primary type registry"
    );
}
