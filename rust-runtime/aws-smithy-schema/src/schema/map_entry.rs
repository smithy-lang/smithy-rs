/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Map entry schema for dynamic map key serialization.

use std::sync::LazyLock;

use crate::{Schema, ShapeId, ShapeType, TraitMap};

/// A schema for map entries with dynamically known keys at runtime.
///
/// During map serialization, each entry's key must be provided as the
/// `member_name()` so that format-specific serializers can write it correctly
/// (e.g., as a JSON object key). Unlike [`MemberSchema`](crate::MemberSchema),
/// which is a `'static` constant generated for struct members whose names are
/// known at compile time, `MapEntrySchema` is stack-allocated per entry during
/// iteration because map keys are only known at runtime.
///
/// # Example
///
/// ```ignore
/// for (key, value) in map {
///     let entry = MapEntrySchema::new(key, ShapeType::String);
///     ser.write_string(&entry, value)?;
/// }
/// ```
pub struct MapEntrySchema<'a> {
    key: &'a str,
    value_type: ShapeType,
}

impl<'a> MapEntrySchema<'a> {
    /// Creates a new map entry schema with the given key and value type.
    pub fn new(key: &'a str, value_type: ShapeType) -> Self {
        Self { key, value_type }
    }
}

impl Schema for MapEntrySchema<'_> {
    fn shape_id(&self) -> &ShapeId {
        static ID: LazyLock<ShapeId> =
            LazyLock::new(|| ShapeId::from_static("smithy.api#MapEntry", "smithy.api", "MapEntry"));
        &ID
    }

    fn shape_type(&self) -> ShapeType {
        self.value_type
    }

    fn traits(&self) -> &TraitMap {
        static MAP: LazyLock<TraitMap> = LazyLock::new(TraitMap::empty);
        &MAP
    }

    fn member_name(&self) -> Option<&str> {
        Some(self.key)
    }
}
