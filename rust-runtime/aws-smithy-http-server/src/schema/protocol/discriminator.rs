/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::{Schema, ShapeId, ShapeType};

/// Member schema for the synthetic `__type` discriminator member.
///
/// The member index is irrelevant on the serialization path (codecs key off
/// `member_name`); `usize::MAX` guards against accidental use for
/// deserialization-side member lookup.
static TYPE_MEMBER: Schema<'static> = Schema::new_member(
    ShapeId::from_parts("smithy.api#String", "smithy.api", "String"),
    ShapeType::String,
    "__type",
    usize::MAX,
);

/// Where the `__type` member sits relative to the error's own members.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TypePosition {
    /// The first map entry (rpcv2Cbor).
    First,
    /// After every modeled member (awsJson 1.0 and 1.1).
    Last,
}

/// What the `__type` member carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TypeValue {
    /// The full `namespace#Name` shape ID (awsJson 1.0, rpcv2Cbor).
    FullShapeId,
    /// The shape name only (awsJson 1.1).
    ShapeName,
}

impl TypeValue {
    pub(super) fn of<'s>(self, schema: &'s Schema<'s>) -> &'s str {
        match self {
            Self::FullShapeId => schema.shape_id().as_str(),
            Self::ShapeName => schema.shape_id().shape_name(),
        }
    }
}

/// How a protocol frames a modeled error's `__type` member in the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BodyDiscriminator {
    pub(super) position: TypePosition,
    pub(super) value: TypeValue,
}

impl BodyDiscriminator {
    /// Wraps `error` so that serializing it also writes the `__type` member.
    pub(super) fn frame<'a>(self, schema: &'a Schema<'a>, error: &'a dyn SerializableStruct) -> WithType<'a> {
        WithType {
            position: self.position,
            type_value: self.value.of(schema),
            inner: error,
        }
    }
}

/// A shape with a synthetic `__type` member spliced into its members.
pub(super) struct WithType<'a> {
    position: TypePosition,
    type_value: &'a str,
    inner: &'a dyn SerializableStruct,
}

impl SerializableStruct for WithType<'_> {
    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        match self.position {
            TypePosition::First => {
                serializer.write_string(&TYPE_MEMBER, self.type_value)?;
                self.inner.serialize_members(serializer)
            }
            TypePosition::Last => {
                self.inner.serialize_members(serializer)?;
                serializer.write_string(&TYPE_MEMBER, self.type_value)
            }
        }
    }
}
