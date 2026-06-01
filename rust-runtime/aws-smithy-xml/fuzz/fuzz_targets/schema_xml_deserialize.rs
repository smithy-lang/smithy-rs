/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![no_main]

use aws_smithy_schema::serde::ShapeDeserializer;
use aws_smithy_schema::{shape_id, Schema, ShapeType};
use aws_smithy_xml::codec::XmlShapeDeserializer;
use libfuzzer_sys::fuzz_target;

static MEMBER_A: Schema = Schema::new_member(shape_id!("t", "S"), ShapeType::String, "A", 0);
static MEMBER_B: Schema = Schema::new_member(shape_id!("t", "S"), ShapeType::Integer, "B", 1);
static MEMBER_C: Schema = Schema::new_member(shape_id!("t", "S"), ShapeType::Structure, "C", 2);
static INNER: Schema =
    Schema::new_struct(shape_id!("t", "Inner"), ShapeType::Structure, &[&MEMBER_A]);
static SCHEMA: Schema = Schema::new_struct(
    shape_id!("t", "S"),
    ShapeType::Structure,
    &[&MEMBER_A, &MEMBER_B, &MEMBER_C],
);

fuzz_target!(|data: &[u8]| {
    let mut deser = XmlShapeDeserializer::new(data);
    let _ = deser.read_struct(&SCHEMA, &mut |member, d| {
        match member.shape_type() {
            ShapeType::String => {
                let _ = d.read_string(member);
            }
            ShapeType::Integer => {
                let _ = d.read_integer(member);
            }
            ShapeType::Structure => {
                let _ = d.read_struct(&INNER, &mut |_, d2| {
                    let _ = d2.read_string(&MEMBER_A);
                    Ok(())
                });
            }
            _ => {}
        }
        Ok(())
    });
    let mut deser = XmlShapeDeserializer::new(data);
    let _ = deser.read_list(&aws_smithy_schema::prelude::STRING, &mut |d| {
        let _ = d.read_string(&aws_smithy_schema::prelude::STRING);
        Ok(())
    });
    let mut deser = XmlShapeDeserializer::new(data);
    let _ = deser.read_map(&aws_smithy_schema::prelude::STRING, &mut |_, d| {
        let _ = d.read_string(&aws_smithy_schema::prelude::STRING);
        Ok(())
    });
    let mut deser = XmlShapeDeserializer::new(data);
    let _ = deser.read_boolean(&aws_smithy_schema::prelude::BOOLEAN);
    let mut deser = XmlShapeDeserializer::new(data);
    let _ = deser.read_float(&aws_smithy_schema::prelude::FLOAT);
    let mut deser = XmlShapeDeserializer::new(data);
    let _ = deser.read_blob(&aws_smithy_schema::prelude::BLOB);
});
