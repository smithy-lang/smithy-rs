/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Storage for [`KnowledgeData`](super::KnowledgeData), keyed by type.

use std::any::{type_name, Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock};

type Erased = Arc<dyn Any + Send + Sync>;

/// One value per type, built on first request and kept until the model is dropped.
///
/// Values are built outside the lock, so a build can request other data. Two threads that
/// request a missing value at the same time may both build it; the first to finish publishes its
/// value and the other's is dropped. A build that requests its own type, directly or through
/// other data, panics instead of recursing.
#[derive(Default)]
pub(crate) struct DataCache {
    values: RwLock<HashMap<TypeId, Erased>>,
}

impl DataCache {
    pub(crate) fn get_or_build<T: Any + Send + Sync>(&self, build: impl FnOnce() -> T) -> Arc<T> {
        let key = TypeId::of::<T>();
        let existing = self
            .values
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
            .cloned();
        if let Some(value) = existing {
            return downcast(value);
        }
        let built: Erased = {
            let _building = Building::enter(self, key, type_name::<T>());
            Arc::new(build())
        };
        let published = Arc::clone(
            self.values
                .write()
                .unwrap_or_else(PoisonError::into_inner)
                .entry(key)
                .or_insert_with(|| Arc::clone(&built)),
        );
        // If another thread published first, `built` is dropped here, outside the lock.
        drop(built);
        downcast(published)
    }
}

fn downcast<T: Any + Send + Sync>(value: Erased) -> Arc<T> {
    value
        .downcast()
        .expect("values are stored under their own type ID")
}

thread_local! {
    /// The data this thread is building, innermost last: the cache's address, the key, and the
    /// type name for the panic message.
    static BUILDING: RefCell<Vec<(usize, TypeId, &'static str)>> = const { RefCell::new(Vec::new()) };
}

/// Marks a value as being built on this thread until dropped, including by a panic.
struct Building;

impl Building {
    fn enter(cache: &DataCache, key: TypeId, name: &'static str) -> Self {
        let cache = std::ptr::from_ref(cache).addr();
        let cycle = BUILDING.with_borrow_mut(|building| {
            let start = building
                .iter()
                .position(|&(c, k, _)| c == cache && k == key)?;
            let chain: Vec<&str> = building[start..]
                .iter()
                .filter(|(c, _, _)| *c == cache)
                .map(|(_, _, name)| *name)
                .chain([name])
                .collect();
            Some(chain.join(" -> "))
        });
        if let Some(cycle) = cycle {
            panic!("knowledge data depends on itself: {cycle}");
        }
        BUILDING.with_borrow_mut(|building| building.push((cache, key, name)));
        Building
    }
}

impl Drop for Building {
    fn drop(&mut self) {
        BUILDING.with_borrow_mut(|building| building.pop());
    }
}
