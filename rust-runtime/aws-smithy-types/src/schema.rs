/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Runtime schema types for Smithy shapes.
//!
//! This module provides the core types for representing Smithy schemas at runtime,
//! enabling protocol-agnostic serialization and deserialization.

mod shape_id;
mod shape_type;
mod trait_map;
mod trait_type;

pub mod prelude;
pub mod serde;

pub use shape_id::ShapeId;
pub use shape_type::ShapeType;
pub use trait_map::TraitMap;
pub use trait_type::Trait;

/// Core trait representing a Smithy schema at runtime.
///
/// A schema is a lightweight runtime representation of a Smithy shape,
/// containing the shape's ID, type, traits, and references to member schemas.
pub trait Schema: Send + Sync {
    /// Returns the Shape ID of this schema.
    fn shape_id(&self) -> &ShapeId;

    /// Returns the shape type.
    fn shape_type(&self) -> ShapeType;

    /// Returns the traits associated with this schema.
    fn traits(&self) -> &TraitMap;

    /// Returns the member name if this is a member schema.
    fn member_name(&self) -> Option<&str> {
        None
    }

    /// Returns the member schema by name (for structures and unions).
    fn member_schema(&self, _name: &str) -> Option<&dyn Schema> {
        None
    }

    /// Returns the member schema by position index (for structures and unions).
    ///
    /// This is an optimization for generated code to avoid string lookups.
    /// Consumer code should not rely on specific position values as they may change.
    fn member_schema_by_index(&self, _index: usize) -> Option<&dyn Schema> {
        None
    }

    /// Returns the member schema for collections (list member or map value).
    fn member(&self) -> Option<&dyn Schema> {
        None
    }

    /// Returns the key schema for maps.
    fn key(&self) -> Option<&dyn Schema> {
        None
    }

    /// Returns an iterator over member schemas (for structures and unions).
    fn members(&self) -> Box<dyn Iterator<Item = &dyn Schema> + '_> {
        Box::new(std::iter::empty())
    }

    /// Returns the member index for member schemas.
    ///
    /// This is used internally by generated code for efficient member lookup.
    /// Returns None if not applicable or not a member schema.
    fn member_index(&self) -> Option<usize> {
        None
    }
}

/// Helper methods for Schema trait.
pub trait SchemaExt: Schema {
    /// Returns true if this is a member schema.
    fn is_member(&self) -> bool {
        self.shape_type().is_member()
    }

    /// Returns true if this is a structure schema.
    fn is_structure(&self) -> bool {
        self.shape_type() == ShapeType::Structure
    }

    /// Returns true if this is a union schema.
    fn is_union(&self) -> bool {
        self.shape_type() == ShapeType::Union
    }

    /// Returns true if this is a list schema.
    fn is_list(&self) -> bool {
        self.shape_type() == ShapeType::List
    }

    /// Returns true if this is a map schema.
    fn is_map(&self) -> bool {
        self.shape_type() == ShapeType::Map
    }

    /// Returns true if this is a blob schema.
    fn is_blob(&self) -> bool {
        self.shape_type() == ShapeType::Blob
    }

    /// Returns true if this is a string schema.
    fn is_string(&self) -> bool {
        self.shape_type() == ShapeType::String
    }
}

impl<T: Schema + ?Sized> SchemaExt for T {}

#[cfg(test)]
mod test {
    use crate::schema::{Schema, SchemaExt, ShapeId, ShapeType, Trait, TraitMap};

    // Simple test trait implementation
    #[derive(Debug)]
    struct TestTrait {
        id: ShapeId,
        #[allow(dead_code)]
        value: String,
    }

    impl Trait for TestTrait {
        fn trait_id(&self) -> &ShapeId {
            &self.id
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    // Simple test schema implementation
    struct TestSchema {
        id: ShapeId,
        shape_type: ShapeType,
        traits: TraitMap,
    }

    impl Schema for TestSchema {
        fn shape_id(&self) -> &ShapeId {
            &self.id
        }

        fn shape_type(&self) -> ShapeType {
            self.shape_type
        }

        fn traits(&self) -> &TraitMap {
            &self.traits
        }
    }

    #[test]
    fn test_shape_type_simple() {
        assert!(ShapeType::String.is_simple());
        assert!(ShapeType::Integer.is_simple());
        assert!(ShapeType::Boolean.is_simple());
        assert!(!ShapeType::Structure.is_simple());
        assert!(!ShapeType::List.is_simple());
    }

    #[test]
    fn test_shape_type_aggregate() {
        assert!(ShapeType::Structure.is_aggregate());
        assert!(ShapeType::Union.is_aggregate());
        assert!(ShapeType::List.is_aggregate());
        assert!(ShapeType::Map.is_aggregate());
        assert!(!ShapeType::String.is_aggregate());
    }

    #[test]
    fn test_shape_type_member() {
        assert!(ShapeType::Member.is_member());
        assert!(!ShapeType::String.is_member());
        assert!(!ShapeType::Structure.is_member());
    }

    #[test]
    fn test_shape_id_parsing() {
        let id = ShapeId::new("smithy.api#String");
        assert_eq!(id.namespace(), "smithy.api");
        assert_eq!(id.shape_name(), "String");
        assert_eq!(id.member_name(), None);
    }

    #[test]
    fn test_shape_id_with_member() {
        let id = ShapeId::new("com.example#MyStruct$member");
        assert_eq!(id.namespace(), "com.example");
        assert_eq!(id.shape_name(), "MyStruct");
        assert_eq!(id.member_name(), Some("member"));
    }

    #[test]
    fn test_trait_map() {
        let mut map = TraitMap::new();
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);

        let trait_id = ShapeId::new("smithy.api#required");
        let test_trait = Box::new(TestTrait {
            id: trait_id.clone(),
            value: "test".to_string(),
        });

        map.insert(test_trait);
        assert!(!map.is_empty());
        assert_eq!(map.len(), 1);
        assert!(map.contains(&trait_id));

        let retrieved = map.get(&trait_id);
        assert!(retrieved.is_some());
    }

    #[test]
    fn test_schema_ext() {
        let schema = TestSchema {
            id: ShapeId::new("com.example#MyStruct"),
            shape_type: ShapeType::Structure,
            traits: TraitMap::new(),
        };

        assert!(schema.is_structure());
        assert!(!schema.is_union());
        assert!(!schema.is_list());
        assert!(!schema.is_member());
    }

    #[test]
    fn test_schema_basic() {
        let schema = TestSchema {
            id: ShapeId::new("smithy.api#String"),
            shape_type: ShapeType::String,
            traits: TraitMap::new(),
        };

        assert_eq!(schema.shape_id().as_str(), "smithy.api#String");
        assert_eq!(schema.shape_type(), ShapeType::String);
        assert!(schema.traits().is_empty());
        assert!(schema.member_name().is_none());
        assert!(schema.member_schema("test").is_none());
        assert!(schema.member_schema_by_index(0).is_none());
    }
}
