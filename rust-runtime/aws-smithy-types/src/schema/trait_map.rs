/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::{ShapeId, Trait};
use std::collections::HashMap;

/// A map of traits keyed by their Shape ID.
///
/// This provides efficient lookup of traits during serialization and deserialization.
#[derive(Debug, Default)]
pub struct TraitMap {
    traits: HashMap<ShapeId, Box<dyn Trait>>,
}

impl TraitMap {
    /// Creates a new empty TraitMap.
    pub fn new() -> Self {
        Self {
            traits: HashMap::new(),
        }
    }

    /// Inserts a trait into the map.
    pub fn insert(&mut self, trait_obj: Box<dyn Trait>) {
        let id = trait_obj.trait_id().clone();
        self.traits.insert(id, trait_obj);
    }

    /// Gets a trait by its Shape ID.
    pub fn get(&self, id: &ShapeId) -> Option<&dyn Trait> {
        self.traits.get(id).map(|t| t.as_ref())
    }

    /// Returns true if the map contains a trait with the given Shape ID.
    pub fn contains(&self, id: &ShapeId) -> bool {
        self.traits.contains_key(id)
    }

    /// Returns an iterator over all traits.
    pub fn iter(&self) -> impl Iterator<Item = &dyn Trait> {
        self.traits.values().map(|t| t.as_ref())
    }

    /// Returns the number of traits in the map.
    pub fn len(&self) -> usize {
        self.traits.len()
    }

    /// Returns true if the map is empty.
    pub fn is_empty(&self) -> bool {
        self.traits.is_empty()
    }
}
