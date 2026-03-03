/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Member schema type for structure/union members and collection elements.

use std::sync::LazyLock;

use crate::{Schema, ShapeId, ShapeType, TraitMap};

/// A schema for a member shape, combining member and target shape information.
///
/// Following the SEP recommendation, this combines the Smithy concepts of
/// member shape and target shape into a single object. The `shape_type()`
/// returns the target shape's type, while `member_name()` and `member_index()`
/// provide member-specific information.
#[derive(Debug)]
pub struct MemberSchema {
    id: ShapeId,
    target_type: ShapeType,
    name: &'static str,
    index: usize,
}

impl MemberSchema {
    /// Creates a new member schema.
    pub const fn new(
        id: ShapeId,
        target_type: ShapeType,
        name: &'static str,
        index: usize,
    ) -> Self {
        Self {
            id,
            target_type,
            name,
            index,
        }
    }
}

impl Schema for MemberSchema {
    fn shape_id(&self) -> &ShapeId {
        &self.id
    }

    fn shape_type(&self) -> ShapeType {
        self.target_type
    }

    fn traits(&self) -> &TraitMap {
        static MAP: LazyLock<TraitMap> = LazyLock::new(TraitMap::empty);
        &MAP
    }

    fn member_name(&self) -> Option<&str> {
        Some(self.name)
    }

    fn member_index(&self) -> Option<usize> {
        Some(self.index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_member_schema() {
        let schema = MemberSchema::new(
            ShapeId::from_static("com.example#MyStruct$name", "com.example", "MyStruct"),
            ShapeType::String,
            "name",
            0,
        );

        assert_eq!(schema.shape_type(), ShapeType::String);
        assert_eq!(schema.member_name(), Some("name"));
        assert_eq!(schema.member_index(), Some(0));
        assert!(schema.traits().is_empty());
    }
}
