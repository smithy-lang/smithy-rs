/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Call forms of `DiscriminatedDocumentExt`, with every item in `aws_smithy_schema::document`
//! imported.
//!
//! The glob import also brings `DiscriminatedDocumentV2Ext` into scope. Its methods have
//! different names, so the path calls below, which name only the type, stay unambiguous. A
//! method with the same name in both traits would make them fail with E0034.

use aws_smithy_schema::document::*;
use aws_smithy_schema::serde::{
    SerdeError, SerializableStruct, ShapeDeserializer, ShapeSerializer,
};
use aws_smithy_schema::{prelude, shape_id, Schema, ShapeType};
use aws_smithy_types::{DiscriminatedDocument, DocumentV2};

static M_NAME: Schema<'static> = Schema::new_member(
    shape_id!("smithy.example", "Person", "name"),
    ShapeType::String,
    "name",
    0,
);
static PERSON_SCHEMA: Schema<'static> = Schema::new_struct(
    shape_id!("smithy.example", "Person"),
    ShapeType::Structure,
    &[&M_NAME],
);

struct Person;

impl SerializableStruct for Person {
    fn schema(&self) -> &Schema<'_> {
        &PERSON_SCHEMA
    }

    fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        ser.write_string(&M_NAME, "Iago")
    }
}

fn read_name(deser: &mut dyn ShapeDeserializer) -> Result<String, SerdeError> {
    let mut name = String::new();
    deser.read_struct(&PERSON_SCHEMA, &mut |member, sub| {
        name = sub.read_string(member)?;
        Ok(())
    })?;
    Ok(name)
}

#[test]
fn released_path_calls_resolve_with_both_traits_in_scope() {
    let document = DiscriminatedDocument::from_struct(&PERSON_SCHEMA, &Person).unwrap();
    assert_eq!(
        DiscriminatedDocument::as_shape(&document, read_name).unwrap(),
        "Iago"
    );
    assert_eq!(DiscriminatedDocument::shape_type(&document), ShapeType::Map);
    assert_eq!(document.as_shape(read_name).unwrap(), "Iago");
}

#[test]
fn v2_path_calls_resolve_with_both_traits_in_scope() {
    let document = DiscriminatedDocument::from_struct_v2(&PERSON_SCHEMA, &Person).unwrap();
    assert_eq!(
        DiscriminatedDocument::as_shape_v2(&document, read_name).unwrap(),
        "Iago"
    );
    assert_eq!(
        DiscriminatedDocument::shape_type_v2(&document),
        ShapeType::Map
    );
    let blob = DiscriminatedDocument::new_v2(DocumentV2::Blob(b"x".to_vec()));
    assert_eq!(blob.shape_type_v2(), ShapeType::Blob);
    assert_eq!(
        blob.as_shape_v2(|deser| deser.read_blob(&prelude::BLOB))
            .unwrap()
            .as_ref(),
        b"x"
    );
}
