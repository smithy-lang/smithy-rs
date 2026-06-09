/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::{ShapeId, Trait};
use std::collections::HashMap;

/// A map of traits keyed by their Shape ID.
///
/// This provides efficient lookup of traits during serialization and deserialization.
#[derive(Debug)]
pub struct TraitMap {
    // Wrapped in `Option` because `HashMap::new()` is not `const fn` in stable Rust,
    // allowing `TraitMap::EMPTY` to be used in const contexts (e.g. prelude schemas).
    //
    // TODO(schema-lifetime): when ShapeId equality across lifetimes is needed
    // (e.g., looking up runtime-built ShapeIds in this map), we'll need a
    // custom Borrow-based lookup or a different keying strategy.
    traits: Option<HashMap<ShapeId<'static>, Box<dyn Trait>>>,
}

impl Default for TraitMap {
    fn default() -> Self {
        Self::new()
    }
}

impl TraitMap {
    /// An empty trait map for use in const contexts (e.g. prelude schemas).
    pub const EMPTY: Self = Self { traits: None };

    /// Creates a new empty TraitMap.
    pub fn new() -> Self {
        Self {
            traits: Some(HashMap::new()),
        }
    }

    /// Inserts a trait into the map.
    pub fn insert(&mut self, trait_obj: Box<dyn Trait>) {
        let id = *trait_obj.trait_id();
        self.traits
            .get_or_insert_with(HashMap::new)
            .insert(id, trait_obj);
    }

    /// Gets a trait by its Shape ID.
    pub fn get(&self, id: &ShapeId<'static>) -> Option<&dyn Trait> {
        self.traits.as_ref()?.get(id).map(|t| t.as_ref())
    }

    /// Returns true if the map contains a trait with the given Shape ID.
    pub fn contains(&self, id: &ShapeId<'static>) -> bool {
        self.traits.as_ref().is_some_and(|m| m.contains_key(id))
    }

    /// Returns the number of traits in the map.
    pub fn len(&self) -> usize {
        self.traits.as_ref().map_or(0, |m| m.len())
    }

    /// Returns true if the map is empty.
    pub fn is_empty(&self) -> bool {
        self.traits.as_ref().is_none_or(|m| m.is_empty())
    }
}
