/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/* Automatically managed default lints */
#![cfg_attr(docsrs, feature(doc_cfg))]
/* End of automatically managed default lints */

//! Runtime schema types for Smithy shapes.
//!
//! This module provides the core types for representing Smithy schemas at runtime,
//! enabling protocol-agnostic serialization and deserialization.

mod schema {
    pub mod shape_id;
    pub mod shape_type;
    pub mod trait_map;
    pub mod trait_type;

    pub mod codec;
    pub mod prelude;
    pub mod serde;
}

pub use schema::shape_id::ShapeId;
pub use schema::shape_type::ShapeType;
pub use schema::trait_map::TraitMap;
pub use schema::trait_type::Trait;

pub mod prelude {
    pub use crate::schema::prelude::*;
}

pub mod serde {
    pub use crate::schema::serde::*;
}

pub mod codec {
    pub use crate::schema::codec::*;
}

/// A Smithy schema — a lightweight runtime representation of a Smithy shape.
///
/// Contains the shape's ID, type, traits relevant to serialization, and
/// references to member schemas (for aggregate types).
///
/// Schemas are constructed at compile time (via `const`) for generated code
/// and prelude types. The Smithy type system is closed, so no extensibility
/// via trait objects is needed.
#[derive(Debug)]
pub struct Schema {
    id: ShapeId,
    shape_type: ShapeType,
    traits: TraitMap,
    /// Member name if this is a member schema.
    member_name: Option<&'static str>,
    /// Member index for position-based lookup in generated code.
    member_index: Option<usize>,
    /// Shape-type-specific member data.
    members: SchemaMembers,
}

/// Shape-type-specific member references.
#[derive(Debug)]
enum SchemaMembers {
    /// No members (simple types).
    None,
    /// Structure or union members.
    Struct { members: &'static [&'static Schema] },
    /// List member schema.
    List { member: &'static Schema },
    /// Map key and value schemas.
    Map {
        key: &'static Schema,
        value: &'static Schema,
    },
}

impl Schema {
    /// Creates a schema for a simple type (no members).
    pub const fn new(id: ShapeId, shape_type: ShapeType, traits: TraitMap) -> Self {
        Self {
            id,
            shape_type,
            traits,
            member_name: None,
            member_index: None,
            members: SchemaMembers::None,
        }
    }

    /// Creates a schema for a structure or union type.
    pub const fn new_struct(
        id: ShapeId,
        shape_type: ShapeType,
        traits: TraitMap,
        members: &'static [&'static Schema],
    ) -> Self {
        Self {
            id,
            shape_type,
            traits,
            member_name: None,
            member_index: None,
            members: SchemaMembers::Struct { members },
        }
    }

    /// Creates a schema for a list type.
    pub const fn new_list(id: ShapeId, traits: TraitMap, member: &'static Schema) -> Self {
        Self {
            id,
            shape_type: ShapeType::List,
            traits,
            member_name: None,
            member_index: None,
            members: SchemaMembers::List { member },
        }
    }

    /// Creates a schema for a map type.
    pub const fn new_map(
        id: ShapeId,
        traits: TraitMap,
        key: &'static Schema,
        value: &'static Schema,
    ) -> Self {
        Self {
            id,
            shape_type: ShapeType::Map,
            traits,
            member_name: None,
            member_index: None,
            members: SchemaMembers::Map { key, value },
        }
    }

    /// Creates a member schema wrapping a target schema.
    pub const fn new_member(
        id: ShapeId,
        shape_type: ShapeType,
        traits: TraitMap,
        member_name: &'static str,
        member_index: usize,
    ) -> Self {
        Self {
            id,
            shape_type,
            traits,
            member_name: Some(member_name),
            member_index: Some(member_index),
            members: SchemaMembers::None,
        }
    }

    /// Returns the Shape ID of this schema.
    pub fn shape_id(&self) -> &ShapeId {
        &self.id
    }

    /// Returns the shape type.
    pub fn shape_type(&self) -> ShapeType {
        self.shape_type
    }

    /// Returns the traits associated with this schema.
    pub fn traits(&self) -> &TraitMap {
        &self.traits
    }

    /// Returns the member name if this is a member schema.
    pub fn member_name(&self) -> Option<&str> {
        self.member_name
    }

    /// Returns the member index for member schemas.
    ///
    /// This is used internally by generated code for efficient member lookup.
    /// Consumer code should not rely on specific position values as they may change.
    pub fn member_index(&self) -> Option<usize> {
        self.member_index
    }

    /// Returns the member schema by name (for structures and unions).
    pub fn member_schema(&self, name: &str) -> Option<&Schema> {
        match &self.members {
            SchemaMembers::Struct { members } => members
                .iter()
                .find(|m| m.member_name == Some(name))
                .copied(),
            _ => None,
        }
    }

    /// Returns the member name and schema by position index (for structures and unions).
    ///
    /// This is an optimization for generated code to avoid string lookups.
    /// Consumer code should not rely on specific position values as they may change.
    pub fn member_schema_by_index(&self, index: usize) -> Option<&Schema> {
        match &self.members {
            SchemaMembers::Struct { members } => members.get(index).copied(),
            _ => None,
        }
    }

    /// Returns the member schemas (for structures and unions).
    pub fn members(&self) -> &[&Schema] {
        match &self.members {
            SchemaMembers::Struct { members } => members,
            _ => &[],
        }
    }

    /// Returns the member schema for collections (list member or map value).
    pub fn member(&self) -> Option<&Schema> {
        match &self.members {
            SchemaMembers::List { member } => Some(member),
            SchemaMembers::Map { value, .. } => Some(value),
            _ => None,
        }
    }

    /// Returns the key schema for maps.
    pub fn key(&self) -> Option<&Schema> {
        match &self.members {
            SchemaMembers::Map { key, .. } => Some(key),
            _ => None,
        }
    }

    // -- convenience predicates --

    /// Returns true if this is a member schema.
    pub fn is_member(&self) -> bool {
        self.shape_type.is_member()
    }

    /// Returns true if this is a structure schema.
    pub fn is_structure(&self) -> bool {
        self.shape_type == ShapeType::Structure
    }

    /// Returns true if this is a union schema.
    pub fn is_union(&self) -> bool {
        self.shape_type == ShapeType::Union
    }

    /// Returns true if this is a list schema.
    pub fn is_list(&self) -> bool {
        self.shape_type == ShapeType::List
    }

    /// Returns true if this is a map schema.
    pub fn is_map(&self) -> bool {
        self.shape_type == ShapeType::Map
    }

    /// Returns true if this is a blob schema.
    pub fn is_blob(&self) -> bool {
        self.shape_type == ShapeType::Blob
    }

    /// Returns true if this is a string schema.
    pub fn is_string(&self) -> bool {
        self.shape_type == ShapeType::String
    }
}

#[cfg(test)]
mod test {
    use crate::{shape_id, Schema, ShapeType, Trait, TraitMap};

    // Simple test trait implementation
    #[derive(Debug)]
    struct TestTrait {
        id: crate::ShapeId,
        #[allow(dead_code)]
        value: String,
    }

    impl Trait for TestTrait {
        fn trait_id(&self) -> &crate::ShapeId {
            &self.id
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
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
        let id = shape_id!("smithy.api", "String");
        assert_eq!(id.namespace(), "smithy.api");
        assert_eq!(id.shape_name(), "String");
        assert_eq!(id.member_name(), None);
    }

    #[test]
    fn test_shape_id_with_member() {
        let id = shape_id!("com.example", "MyStruct", "member");
        assert_eq!(id.namespace(), "com.example");
        assert_eq!(id.shape_name(), "MyStruct");
        assert_eq!(id.member_name(), Some("member"));
    }

    #[test]
    fn test_trait_map() {
        let mut map = TraitMap::new();
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);

        let trait_id = shape_id!("smithy.api", "required");
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
    fn test_schema_predicates() {
        let schema = Schema::new(
            shape_id!("com.example", "MyStruct"),
            ShapeType::Structure,
            TraitMap::new(),
        );

        assert!(schema.is_structure());
        assert!(!schema.is_union());
        assert!(!schema.is_list());
        assert!(!schema.is_member());
    }

    #[test]
    fn test_schema_basic() {
        let schema = Schema::new(
            shape_id!("smithy.api", "String"),
            ShapeType::String,
            TraitMap::new(),
        );

        assert_eq!(schema.shape_id().as_str(), "smithy.api#String");
        assert_eq!(schema.shape_type(), ShapeType::String);
        assert!(schema.traits().is_empty());
        assert!(schema.member_name().is_none());
        assert!(schema.member_schema("test").is_none());
        assert!(schema.member_schema_by_index(0).is_none());
    }
}
