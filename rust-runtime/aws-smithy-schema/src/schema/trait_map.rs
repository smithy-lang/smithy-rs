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
    traits: HashMap<ShapeId, Box<dyn Trait>>,
}

impl Default for TraitMap {
    fn default() -> Self {
        Self::new()
    }
}

impl TraitMap {
    // TODO(schema) Is there a reasonable with_capacity size for this?
    /// Creates a new empty TraitMap.
    pub fn new() -> Self {
        Self {
            traits: HashMap::new(),
        }
    }

    /// Creates a TraitMap with zero allocated space for Prelude Schemas.
    pub(crate) fn empty() -> Self {
        Self {
            traits: HashMap::with_capacity(0),
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
    pub fn iter(&self) -> impl Iterator<Item = (&ShapeId, &dyn Trait)> {
        self.traits.iter().map(|(s, t)| (s, t.as_ref()))
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
