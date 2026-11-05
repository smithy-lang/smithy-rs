/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! A schema built in a batch may not borrow a value from outside the batch that is not the
//! storage's, even one that outlives the call, because the storage may outlive it (INV-4 in
//! `owner.rs`).

#![allow(dead_code)]

// `owner.rs` names `crate::Model`.
use aws_smithy_lang::Model;
use aws_smithy_schema::{shape_id, Schema, ShapeType};

#[path = "../../src/runtime_schema/owner.rs"]
mod owner;

fn main() {
    let model = Model::from_json_str("m.json", r#"{"smithy": "2.0"}"#).unwrap();
    let storage = owner::Storage::new(model);
    let name = String::from("captured");
    let _handle = storage.with_builder(|_, builder| {
        let member = Schema::new_member(shape_id!("ns", "S", "m"), ShapeType::String, &name, 0);
        //~^ E0597
        builder.alloc(member).0
    });
}
