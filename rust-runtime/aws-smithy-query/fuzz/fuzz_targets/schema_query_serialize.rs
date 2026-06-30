/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![no_main]

use arbitrary::Arbitrary;
use aws_smithy_query::codec::serializer::QueryShapeSerializer;
use aws_smithy_schema::codec::FinishSerializer;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::{shape_id, Schema, ShapeType};
use libfuzzer_sys::fuzz_target;

static STR_MEMBER: Schema = Schema::new_member(shape_id!("t", "S"), ShapeType::String, "Str", 0);
static INT_MEMBER: Schema = Schema::new_member(shape_id!("t", "S"), ShapeType::Integer, "Int", 1);
static LIST_MEMBER: Schema = Schema::new_member(shape_id!("t", "S"), ShapeType::List, "List", 2);
static SCHEMA: Schema = Schema::new_struct(
    shape_id!("t", "S"),
    ShapeType::Structure,
    &[&STR_MEMBER, &INT_MEMBER, &LIST_MEMBER],
);

#[derive(Arbitrary, Debug)]
struct FuzzInput {
    action: String,
    version: String,
    str_val: String,
    int_val: i32,
    list_vals: Vec<String>,
}

struct FuzzStruct<'a>(&'a FuzzInput);

impl SerializableStruct for FuzzStruct<'_> {
    fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        s.write_string(&STR_MEMBER, &self.0.str_val)?;
        s.write_integer(&INT_MEMBER, self.0.int_val)?;
        if !self.0.list_vals.is_empty() {
            s.write_list(&LIST_MEMBER, &|s| {
                for v in &self.0.list_vals {
                    s.write_string(&aws_smithy_schema::prelude::STRING, v)?;
                }
                Ok(())
            })?;
        }
        Ok(())
    }
}

fuzz_target!(|input: FuzzInput| {
    let mut ser = QueryShapeSerializer::new(&input.action, &input.version);
    let _ = ser.write_struct(&SCHEMA, &FuzzStruct(&input));
    let output = ser.finish();
    // Output must be valid UTF-8 (URL-encoded form data)
    assert!(std::str::from_utf8(&output).is_ok());
});
