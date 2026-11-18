/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! A reference to a schema built in a batch cannot leave the batch; only a handle can
//! (INV-4 in `owner.rs`).

#![allow(dead_code)]

// `owner.rs` names `crate::Model`.
use aws_smithy_lang::Model;
use aws_smithy_schema::{shape_id, Schema, ShapeType};

#[path = "../../src/runtime_schema/owner.rs"]
mod owner;

fn main() {
    let model = Model::from_json_str("m.json", r#"{"smithy": "2.0"}"#).unwrap();
    let storage = owner::Storage::new(model);
    let _schema = storage.with_builder(|_, builder| {
        let (_, schema) = builder.alloc(Schema::new(shape_id!("ns", "S"), ShapeType::String));
        schema
        //~^ ERROR: lifetime may not live long enough
    });
}
