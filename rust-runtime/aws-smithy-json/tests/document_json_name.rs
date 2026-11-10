/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Reading typed shapes from JSON documents whose members are renamed with `@jsonName`.
//!
//! A document that `JsonDeserializer` reads keeps the object keys from the wire and carries the
//! codec's settings. Reading a structure from it, through `as_shape`, `as_shape_v2`, or
//! `TypeRegistry::deserialize_document`, must match those keys to members exactly as the codec
//! matches the fields of a structure it reads directly.

use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
use aws_smithy_schema::codec::Codec;
use aws_smithy_schema::document::{DiscriminatedDocumentExt, DiscriminatedDocumentV2Ext};
use aws_smithy_schema::registry::TypeRegistry;
use aws_smithy_schema::serde::{
    SerdeError, SerializableStruct, ShapeDeserializer, ShapeSerializer,
};
use aws_smithy_schema::{shape_id, Schema, ShapeType};
use aws_smithy_types::type_erasure::TypeErasedBox;
use aws_smithy_types::{DiscriminatedDocument, DocumentV2};

static NICKNAME: Schema<'static> = Schema::new_member(
    shape_id!("smithy.example", "Pet", "nickname"),
    ShapeType::String,
    "nickname",
    0,
)
.with_json_name("Nickname");
// `first` and `second` swap names on the wire, which Smithy allows.
static FIRST: Schema<'static> = Schema::new_member(
    shape_id!("smithy.example", "Pet", "first"),
    ShapeType::String,
    "first",
    1,
)
.with_json_name("second");
static SECOND: Schema<'static> = Schema::new_member(
    shape_id!("smithy.example", "Pet", "second"),
    ShapeType::String,
    "second",
    2,
)
.with_json_name("first");
static PLAIN: Schema<'static> = Schema::new_member(
    shape_id!("smithy.example", "Pet", "plain"),
    ShapeType::String,
    "plain",
    3,
);
static OWNER: Schema<'static> = Schema::new_member(
    shape_id!("smithy.example", "Pet", "owner"),
    ShapeType::Structure,
    "owner",
    4,
);
static PET_SCHEMA: Schema<'static> = Schema::new_struct(
    shape_id!("smithy.example", "Pet"),
    ShapeType::Structure,
    &[&NICKNAME, &FIRST, &SECOND, &PLAIN, &OWNER],
);

static OWNER_NAME: Schema<'static> = Schema::new_member(
    shape_id!("smithy.example", "Owner", "name"),
    ShapeType::String,
    "name",
    0,
)
.with_json_name("Name");
static OWNER_SCHEMA: Schema<'static> = Schema::new_struct(
    shape_id!("smithy.example", "Owner"),
    ShapeType::Structure,
    &[&OWNER_NAME],
);

#[derive(Debug, Default, Clone, PartialEq)]
struct Pet {
    nickname: Option<String>,
    first: Option<String>,
    second: Option<String>,
    plain: Option<String>,
    owner: Option<Owner>,
}

#[derive(Debug, Default, Clone, PartialEq)]
struct Owner {
    name: Option<String>,
}

impl SerializableStruct for Pet {
    fn schema(&self) -> &Schema<'_> {
        &PET_SCHEMA
    }

    fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        for (member, value) in [
            (&NICKNAME, &self.nickname),
            (&FIRST, &self.first),
            (&SECOND, &self.second),
            (&PLAIN, &self.plain),
        ] {
            if let Some(value) = value {
                ser.write_string(member, value)?;
            }
        }
        if let Some(owner) = &self.owner {
            ser.write_struct(&OWNER, owner)?;
        }
        Ok(())
    }
}

impl SerializableStruct for Owner {
    fn schema(&self) -> &Schema<'_> {
        &OWNER_SCHEMA
    }

    fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        if let Some(name) = &self.name {
            ser.write_string(&OWNER_NAME, name)?;
        }
        Ok(())
    }
}

impl Pet {
    fn deserialize(deser: &mut dyn ShapeDeserializer) -> Result<Self, SerdeError> {
        let mut pet = Pet::default();
        deser.read_struct(&PET_SCHEMA, &mut |member, sub| {
            match member.member_index() {
                Some(0) => pet.nickname = Some(sub.read_string(member)?),
                Some(1) => pet.first = Some(sub.read_string(member)?),
                Some(2) => pet.second = Some(sub.read_string(member)?),
                Some(3) => pet.plain = Some(sub.read_string(member)?),
                Some(4) => pet.owner = Some(Owner::deserialize(sub)?),
                _ => {}
            }
            Ok(())
        })?;
        Ok(pet)
    }
}

impl Owner {
    fn deserialize(deser: &mut dyn ShapeDeserializer) -> Result<Self, SerdeError> {
        let mut owner = Owner::default();
        deser.read_struct(&OWNER_SCHEMA, &mut |member, sub| {
            if member.member_index() == Some(0) {
                owner.name = Some(sub.read_string(member)?);
            }
            Ok(())
        })?;
        Ok(owner)
    }
}

fn deserialize_pet(deser: &mut dyn ShapeDeserializer) -> Result<TypeErasedBox, SerdeError> {
    Ok(TypeErasedBox::new(Pet::deserialize(deser)?))
}

fn registry() -> TypeRegistry {
    TypeRegistry::builder()
        .insert_shape(&PET_SCHEMA, deserialize_pet)
        .build()
}

fn codec(use_json_name: bool) -> JsonCodec {
    JsonCodec::new(
        JsonCodecSettings::builder()
            .use_json_name(use_json_name)
            .build(),
    )
}

fn pet() -> Pet {
    Pet {
        nickname: Some("Rex".to_string()),
        first: Some("one".to_string()),
        second: Some("two".to_string()),
        plain: Some("plain".to_string()),
        owner: Some(Owner {
            name: Some("Sam".to_string()),
        }),
    }
}

/// Reads `bytes` as a V1 and a V2 discriminated document, and returns the `Pet` that the type
/// registry reads from each.
fn pets_from_documents(codec: &JsonCodec, bytes: &[u8]) -> [Pet; 2] {
    let v1: DiscriminatedDocument = codec
        .create_deserializer(bytes)
        .read_discriminated_document()
        .unwrap();
    let v2: DiscriminatedDocument<DocumentV2> = codec
        .create_deserializer(bytes)
        .read_discriminated_document_v2()
        .unwrap();
    assert_eq!(v1.discriminator(), Some("smithy.example#Pet"));
    assert_eq!(v2.discriminator(), Some("smithy.example#Pet"));
    let registry = registry();
    [
        *registry
            .deserialize_document(&v1)
            .unwrap()
            .downcast::<Pet>()
            .unwrap(),
        *registry
            .deserialize_document(&v2)
            .unwrap()
            .downcast::<Pet>()
            .unwrap(),
    ]
}

/// Adds a `__type` discriminator to a serialized structure.
fn with_type(bytes: &[u8]) -> Vec<u8> {
    let object = std::str::from_utf8(bytes).unwrap();
    let members = object.strip_prefix('{').unwrap();
    format!(r#"{{"__type":"smithy.example#Pet",{members}"#).into_bytes()
}

#[test]
fn registry_reads_documents_with_the_codecs_member_names() {
    for (use_json_name, expected) in [
        (
            true,
            r#"{"Nickname":"Rex","second":"one","first":"two","plain":"plain","owner":{"Name":"Sam"}}"#,
        ),
        (
            false,
            r#"{"nickname":"Rex","first":"one","second":"two","plain":"plain","owner":{"name":"Sam"}}"#,
        ),
    ] {
        let codec = codec(use_json_name);
        let mut ser = codec.create_serializer();
        ser.write_struct(&PET_SCHEMA, &pet()).unwrap();
        let bytes = ser.finish();
        assert_eq!(std::str::from_utf8(&bytes).unwrap(), expected);

        let bytes = with_type(&bytes);
        assert_eq!(
            pets_from_documents(&codec, &bytes),
            [pet(), pet()],
            "use_json_name: {use_json_name}"
        );
    }
}

#[test]
fn document_reads_resolve_keys_like_the_codec() {
    // Every renamed member appears under both its member name and its `@jsonName`, and each
    // value names the key it came from.
    let bytes = br#"{
        "__type": "smithy.example#Pet",
        "nickname": "nickname key", "Nickname": "Nickname key",
        "first": "first key", "second": "second key",
        "plain": "plain key",
        "owner": {"name": "name key", "Name": "Name key"}
    }"#;
    for (use_json_name, expected) in [
        (
            true,
            Pet {
                nickname: Some("Nickname key".to_string()),
                first: Some("second key".to_string()),
                second: Some("first key".to_string()),
                plain: Some("plain key".to_string()),
                owner: Some(Owner {
                    name: Some("Name key".to_string()),
                }),
            },
        ),
        (
            false,
            Pet {
                nickname: Some("nickname key".to_string()),
                first: Some("first key".to_string()),
                second: Some("second key".to_string()),
                plain: Some("plain key".to_string()),
                owner: Some(Owner {
                    name: Some("name key".to_string()),
                }),
            },
        ),
    ] {
        let codec = codec(use_json_name);
        let direct = Pet::deserialize(&mut codec.create_deserializer(bytes)).unwrap();
        assert_eq!(direct, expected, "use_json_name: {use_json_name}");
        assert_eq!(
            pets_from_documents(&codec, bytes),
            [expected.clone(), expected],
            "use_json_name: {use_json_name}"
        );
    }
}

#[test]
fn documents_from_a_data_object_use_member_names() {
    // A document built from a data object has member-name keys and no settings, whatever
    // codec later writes it.
    let v1 = DiscriminatedDocument::from_struct(&PET_SCHEMA, &pet()).unwrap();
    let v2 = DiscriminatedDocument::from_struct_v2(&PET_SCHEMA, &pet()).unwrap();
    assert!(v1.settings().is_none() && v2.settings().is_none());
    assert!(v1.document().as_object().unwrap().contains_key("nickname"));
    assert!(v2.document().as_object().unwrap().contains_key("nickname"));
    let registry = registry();
    for typed in [
        registry.deserialize_document(&v1).unwrap(),
        registry.deserialize_document(&v2).unwrap(),
    ] {
        assert_eq!(*typed.downcast::<Pet>().unwrap(), pet());
    }
    assert_eq!(v1.as_shape(Pet::deserialize).unwrap(), pet());
    assert_eq!(v2.as_shape_v2(Pet::deserialize).unwrap(), pet());
}
