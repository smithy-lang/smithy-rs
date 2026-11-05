/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//@check-pass
//! A schema built in a batch may borrow the storage's data (INV-4 in `owner.rs`).
//!
//! This also keeps the other files in this directory honest: if `owner.rs` stops compiling on
//! its own here, this test fails instead of the compile-fail tests passing for the wrong reason.

#![allow(dead_code)]

// `owner.rs` names `crate::Model`.
use aws_smithy_lang::Model;
use aws_smithy_schema::{Schema, ShapeId, ShapeType};

#[path = "../../src/runtime_schema/owner.rs"]
mod owner;

fn main() {
    let model = Model::from_json_str(
        "m.json",
        r#"{"smithy": "2.0", "shapes": {"ns#S": {"type": "string"}}}"#,
    )
    .unwrap();
    let storage = owner::Storage::new(model);
    let _handle = storage.with_builder(|model, builder| {
        let id = model.shapes().next().unwrap().id();
        let id = ShapeId::from_parts(id.as_str(), id.namespace(), id.name());
        builder.alloc(Schema::new(id, ShapeType::String)).0
    });
}
