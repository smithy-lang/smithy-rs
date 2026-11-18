/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Reading epoch-seconds timestamps from `DocumentV2` values that the JSON codec reads.
//!
//! The JSON codec writes a timestamp with nanoseconds as `1700000000.123456789`. No `f64` holds
//! that number exactly, so `read_document_v2` reads it as a `DocumentV2::BigDecimal`. Reading a
//! timestamp from that document must give the original timestamp, and must succeed wherever
//! reading one from the `Document` for the same JSON does.

use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
use aws_smithy_schema::codec::Codec;
use aws_smithy_schema::document::{DiscriminatedDocumentExt, DiscriminatedDocumentV2Ext};
use aws_smithy_schema::prelude::{DOCUMENT, TIMESTAMP};
use aws_smithy_schema::registry::{RegistryEntry, TypeRegistry};
use aws_smithy_schema::serde::{
    SerdeError, SerializableStruct, ShapeDeserializer, ShapeSerializer,
};
use aws_smithy_schema::{shape_id, Schema, ShapeType};
use aws_smithy_types::type_erasure::TypeErasedBox;
use aws_smithy_types::{DateTime, DiscriminatedDocument, DocumentV2};
use std::sync::Arc;

static AT: Schema<'static> = Schema::new_member(
    shape_id!("smithy.example", "Event", "at"),
    ShapeType::Timestamp,
    "at",
    0,
);
static EVENT_SCHEMA: Schema<'static> = Schema::new_struct(
    shape_id!("smithy.example", "Event"),
    ShapeType::Structure,
    &[&AT],
);

#[derive(Debug, Default, Clone, PartialEq)]
struct Event {
    at: Option<DateTime>,
}

impl SerializableStruct for Event {
    fn schema(&self) -> &Schema<'_> {
        &EVENT_SCHEMA
    }

    fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        if let Some(at) = &self.at {
            ser.write_timestamp(&AT, at)?;
        }
        Ok(())
    }
}

fn deserialize_event(deser: &mut dyn ShapeDeserializer) -> Result<TypeErasedBox, SerdeError> {
    let mut event = Event::default();
    deser.read_struct(&EVENT_SCHEMA, &mut |member, sub| {
        if member.member_index() == Some(0) {
            event.at = Some(sub.read_timestamp(member)?);
        }
        Ok(())
    })?;
    Ok(TypeErasedBox::new(event))
}

fn registry() -> TypeRegistry {
    TypeRegistry::builder()
        .insert(
            EVENT_SCHEMA.shape_id().clone(),
            RegistryEntry::new(&EVENT_SCHEMA, deserialize_event).with_serializable::<Event>(),
        )
        .build()
}

fn nanosecond_timestamp() -> DateTime {
    DateTime::from_secs_and_nanos(1_700_000_000, 123_456_789)
}

#[test]
fn nanosecond_timestamp_reads_back() {
    let at = nanosecond_timestamp();
    let codec = JsonCodec::new(JsonCodecSettings::default());
    let mut ser = codec.create_serializer();
    ser.write_document_v2(&DOCUMENT, &DocumentV2::Timestamp(at))
        .unwrap();
    let wire = ser.finish();
    assert_eq!(wire, b"1700000000.123456789");

    let read = codec
        .create_deserializer(&wire)
        .read_document_v2(&DOCUMENT)
        .unwrap();
    assert!(matches!(read, DocumentV2::BigDecimal(_)), "{read:?}");
    let document =
        DiscriminatedDocument::new_v2(read).with_settings(Arc::new(JsonCodecSettings::default()));
    assert_eq!(
        document
            .as_shape_v2(|d| d.read_timestamp(&TIMESTAMP))
            .unwrap(),
        at
    );
    assert_eq!(document.as_timestamp().unwrap(), at);
}

#[test]
fn registry_round_trips_a_nanosecond_timestamp() {
    let codec = JsonCodec::new(JsonCodecSettings::default());
    let event = Event {
        at: Some(nanosecond_timestamp()),
    };
    let mut ser = codec.create_serializer();
    ser.write_typed_document(&DOCUMENT, &event).unwrap();
    let wire = ser.finish();
    assert_eq!(
        std::str::from_utf8(&wire).unwrap(),
        r#"{"__type":"smithy.example#Event","at":1700000000.123456789}"#
    );

    let document: DiscriminatedDocument<DocumentV2> = codec
        .create_deserializer(&wire)
        .read_discriminated_document_v2()
        .unwrap();
    let registry = registry();
    let read = registry
        .deserialize_document(&document)
        .unwrap()
        .downcast::<Event>()
        .unwrap();
    assert_eq!(*read, event);

    let mut ser = codec.create_serializer();
    registry
        .serialize_document(&mut ser, &DOCUMENT, &document)
        .unwrap();
    assert_eq!(ser.finish(), wire);
}

#[test]
fn v2_documents_read_a_timestamp_wherever_v1_documents_do() {
    let codec = JsonCodec::new(JsonCodecSettings::default());
    for text in [
        "0",
        "-5",
        "1.5",
        "-1.5",
        "1700000000.123456789",
        "-1700000000.123456789",
        // More than nine fraction digits, and exponents, aren't exact epoch-seconds text.
        "1700000000.1234567891",
        "17000000001234567891e-10",
        "1.7e9",
    ] {
        let v1 = DiscriminatedDocument::new(
            codec
                .create_deserializer(text.as_bytes())
                .read_document(&DOCUMENT)
                .unwrap(),
        )
        .with_settings(Arc::new(JsonCodecSettings::default()));
        let v2 = DiscriminatedDocument::new_v2(
            codec
                .create_deserializer(text.as_bytes())
                .read_document_v2(&DOCUMENT)
                .unwrap(),
        )
        .with_settings(Arc::new(JsonCodecSettings::default()));

        let from_v1 = v1.as_shape(|d| d.read_timestamp(&TIMESTAMP)).unwrap();
        let from_v2 = v2.as_shape_v2(|d| d.read_timestamp(&TIMESTAMP)).unwrap();
        assert_eq!(v1.as_timestamp().unwrap(), from_v1, "{text}");
        assert_eq!(v2.as_timestamp().unwrap(), from_v2, "{text}");

        // `f64` holds nanoseconds only to about 100ns at this magnitude, so V2 can be more
        // precise than V1, but not far from it.
        let difference = (from_v2.as_nanos() - from_v1.as_nanos()).abs();
        assert!(difference < 1_000, "{text}: {from_v1:?} and {from_v2:?}");
    }
}
