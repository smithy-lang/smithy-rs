/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! A builder cannot be converted to one with a longer lifetime, whose references would outlive
//! the storage (INV-4 in `owner.rs`). This holds while `Builder` is invariant or covariant in
//! `'b`.

#![allow(dead_code)]

// `owner.rs` names `crate::Model`.
use aws_smithy_lang::Model;
use aws_smithy_schema::{shape_id, Schema, ShapeType};

#[path = "../../src/runtime_schema/owner.rs"]
mod owner;

fn main() {
    let model = Model::from_json_str("m.json", r#"{"smithy": "2.0"}"#).unwrap();
    let storage = owner::Storage::new(model);
    let schema: &'static Schema<'static> = storage.with_builder(|_, builder| {
        let builder: owner::Builder<'static> = builder;
        //~^ ERROR: lifetime may not live long enough
        builder
            .alloc(Schema::new(shape_id!("ns", "S"), ShapeType::String))
            .1
    });
    drop(storage);
    let _ = schema;
}
