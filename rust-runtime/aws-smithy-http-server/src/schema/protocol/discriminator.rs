/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::{Schema, ShapeId, ShapeType};

// ============================================================================
// Discriminator injection
// ============================================================================

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

/// Wrapper prepending a synthetic `__type` member before the inner shape's
/// members (rpcv2Cbor: `__type` is the first map entry).
pub(super) struct WithTypeFirst<'a> {
    pub(super) type_value: &'a str,
    pub(super) inner: &'a dyn SerializableStruct,
}

impl SerializableStruct for WithTypeFirst<'_> {
    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        serializer.write_string(&TYPE_MEMBER, self.type_value)?;
        self.inner.serialize_members(serializer)
    }
}

/// Wrapper appending a synthetic `__type` member after the inner shape's
/// members (awsJson 1.0 / 1.1: `__type` is written last, matching the legacy
/// generated serializers).
pub(super) struct WithTypeLast<'a> {
    pub(super) type_value: &'a str,
    pub(super) inner: &'a dyn SerializableStruct,
}

impl SerializableStruct for WithTypeLast<'_> {
    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        self.inner.serialize_members(serializer)?;
        serializer.write_string(&TYPE_MEMBER, self.type_value)
    }
}

/// awsJson 1.0 discriminator: the full `namespace#Name` shape ID.
pub(super) fn full_shape_id<'s>(schema: &'s Schema<'s>) -> &'s str {
    schema.shape_id().as_str()
}

/// awsJson 1.1 discriminator: the shape name only.
pub(super) fn shape_name_only<'s>(schema: &'s Schema<'s>) -> &'s str {
    schema.shape_id().shape_name()
}
