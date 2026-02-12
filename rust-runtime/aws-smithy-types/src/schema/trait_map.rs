/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::{ShapeId, Trait};
use std::collections::HashMap;

/// A map of traits keyed by their Shape ID.
///
/// This provides efficient lookup of traits during serialization and deserialization.
#[derive(Debug)]
pub struct TraitMap {
    traits: Option<HashMap<ShapeId, Box<dyn Trait>>>,
}

impl Default for TraitMap {
    fn default() -> Self {
        Self::new()
    }
}

impl TraitMap {
    /// Creates a new empty TraitMap.
    pub fn new() -> Self {
        Self {
            traits: Some(HashMap::new()),
        }
    }

    /// Creates an empty TraitMap for const contexts.
    pub const fn empty() -> Self {
        Self { traits: None }
    }

    /// Inserts a trait into the map.
    pub fn insert(&mut self, trait_obj: Box<dyn Trait>) {
        if self.traits.is_none() {
            self.traits = Some(HashMap::new());
        }
        let id = trait_obj.trait_id().clone();
        self.traits.as_mut().unwrap().insert(id, trait_obj);
    }

    /// Gets a trait by its Shape ID.
    pub fn get(&self, id: &ShapeId) -> Option<&dyn Trait> {
        self.traits.as_ref()?.get(id).map(|t| t.as_ref())
    }

    /// Returns true if the map contains a trait with the given Shape ID.
    pub fn contains(&self, id: &ShapeId) -> bool {
        self.traits
            .as_ref()
            .map(|m| m.contains_key(id))
            .unwrap_or(false)
    }

    /// Returns an iterator over all traits.
    pub fn iter(&self) -> impl Iterator<Item = &dyn Trait> {
        self.traits
            .as_ref()
            .into_iter()
            .flat_map(|m| m.values().map(|t| t.as_ref()))
    }

    /// Returns the number of traits in the map.
    pub fn len(&self) -> usize {
        self.traits.as_ref().map(|m| m.len()).unwrap_or(0)
    }

    /// Returns true if the map is empty.
    pub fn is_empty(&self) -> bool {
        self.traits.as_ref().map(|m| m.is_empty()).unwrap_or(true)
    }
}
