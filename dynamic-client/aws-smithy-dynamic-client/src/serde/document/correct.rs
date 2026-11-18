/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// This module fills the members a response left out, as generated clients do when they build an
// output.
//
// A member with `@default` gets its default value. Otherwise, a `@required` member gets the zero
// value of its type, so a caller can read every required member of an output.

use super::{model_member, target_schema};
use aws_smithy_lang::node::{Node, NodeValue};
use aws_smithy_lang::runtime_schema::RuntimeSchemaSet;
use aws_smithy_lang::shape::{MemberView, ShapeType as ModelShapeType};
use aws_smithy_lang::ShapeId;
use aws_smithy_schema::Schema;
use aws_smithy_types::{Document, Number};
use std::collections::HashMap;

const REQUIRED: ShapeId = ShapeId::from_static("smithy.api#required");
const DEFAULT: ShapeId = ShapeId::from_static("smithy.api#default");
const ENUM: ShapeId = ShapeId::from_static("smithy.api#enum");

// Fills the members of `schema`, a structure or union schema, that `members` leaves out.
// TODO(aws-smithy-schema): Compute the fill values once per structure, and keep them in a schema
// extension, once schema extensions are safe on recursive schemas.
pub(super) fn fill_missing(
    schemas: &RuntimeSchemaSet,
    schema: &Schema<'_>,
    members: &mut HashMap<String, Document>,
) {
    if schema.is_union() {
        return;
    }
    for member_schema in schema.members() {
        let name = member_schema
            .member_name()
            .expect("structure members have names");
        if members.contains_key(name) || member_schema.streaming() {
            continue;
        }
        let Some(member) = model_member(schemas, member_schema) else {
            continue;
        };
        // TODO(aws-smithy-lang): Ask a `NullableIndex` whether the member is nullable and what its
        // default is, once one exists. It also covers `@clientOptional` and `@addedDefault`.
        let value = match member.get_trait(&DEFAULT) {
            Some(default) if !default.is_null() => default_value(&member, default),
            _ if member.has_trait(&REQUIRED) => zero_value(schemas, member_schema, &member),
            _ => None,
        };
        if let Some(value) = value {
            members.insert(name.to_owned(), value);
        }
    }
}

// Returns the value of a member's `@default` trait.
fn default_value(member: &MemberView<'_>, default: &Node) -> Option<Document> {
    match member.target_shape().shape_type() {
        // TODO(dynamic-client): Fill blob and timestamp defaults once outputs are DocumentV2.
        ModelShapeType::Blob | ModelShapeType::Timestamp => None,
        _ => Some(document(default)),
    }
}

// Returns the zero value of a required member, as `ErrorCorrection.kt` gives the value.
fn zero_value(
    schemas: &RuntimeSchemaSet,
    schema: &Schema<'_>,
    member: &MemberView<'_>,
) -> Option<Document> {
    let target = member.target_shape();
    let value = match target.shape_type() {
        // TODO(aws-smithy-lang): Use `ShapeView::is_enum()`, once one exists, for both enum forms.
        ModelShapeType::Enum => Document::String("no value was set".to_owned()),
        ModelShapeType::String if target.has_trait(&ENUM) => {
            Document::String("no value was set".to_owned())
        }
        ModelShapeType::String => Document::String(String::new()),
        ModelShapeType::Boolean => Document::Bool(false),
        ModelShapeType::Byte
        | ModelShapeType::Short
        | ModelShapeType::Integer
        | ModelShapeType::IntEnum
        | ModelShapeType::Long => Document::Number(Number::PosInt(0)),
        ModelShapeType::Float | ModelShapeType::Double => Document::Number(Number::Float(0.0)),
        ModelShapeType::Document => Document::Null,
        ModelShapeType::List => Document::Array(Vec::new()),
        ModelShapeType::Map => Document::Object(HashMap::new()),
        ModelShapeType::Structure => {
            let target = target_schema(schemas, schema).ok()?;
            let mut members = HashMap::new();
            fill_missing(schemas, target, &mut members);
            Document::Object(members)
        }
        // A document has no unknown union variant.
        ModelShapeType::Union => return None,
        // TODO(dynamic-client): Fill blob, timestamp, and big number zero values once outputs
        // are DocumentV2.
        _ => return None,
    };
    Some(value)
}

// TODO(aws-smithy-lang): Use a `Node` to document conversion behind the `runtime-schema` feature,
// once one exists. With DocumentV2, it takes the target type, so blob and timestamp values work.
fn document(node: &Node) -> Document {
    match node.value() {
        NodeValue::Null => Document::Null,
        NodeValue::Bool(value) => Document::Bool(*value),
        NodeValue::Number(number) => Document::Number(match (number.as_u64(), number.as_i64()) {
            (Some(value), _) if !number.is_float() => Number::PosInt(value),
            (_, Some(value)) if !number.is_float() => Number::NegInt(value),
            _ => Number::Float(number.as_f64()),
        }),
        NodeValue::String(value) => Document::String(value.clone()),
        NodeValue::Array(items) => Document::Array(items.iter().map(document).collect()),
        NodeValue::Object(entries) => Document::Object(
            entries
                .iter()
                .map(|(key, value)| (key.to_owned(), document(value)))
                .collect(),
        ),
    }
}
