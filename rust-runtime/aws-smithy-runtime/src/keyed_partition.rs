/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// static TOKEN_BUCKET: #{OnceCell}<#{TokenBucket}> = #{OnceCell}::new();

use once_cell::sync::OnceCell;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Arc, Mutex, MutexGuard};

pub struct KeyedPartition<K, V> {
    inner: OnceCell<Mutex<HashMap<K, Arc<V>>>>,
}

impl<K, V> KeyedPartition<K, V> {
    pub const fn new() -> Self {
        // At the very least, we'll always be storing the default state.
        Self {
            inner: OnceCell::new(),
        }
    }
}

impl<K, V> KeyedPartition<K, V>
where
    K: Eq + Hash,
{
    fn get_or_init_inner(&self) -> MutexGuard<'_, HashMap<K, Arc<V>>> {
        self.inner
            .get_or_init(|| Mutex::new(HashMap::with_capacity(1)))
            .lock()
            .unwrap()
    }

    #[must_use]
    pub fn get(&self, partition_key: K) -> Option<Arc<V>> {
        self.get_or_init_inner().get(&partition_key).cloned()
    }

    #[must_use]
    pub fn get_or_init<F>(&self, partition_key: K, f: F) -> Arc<V>
    where
        F: FnOnce() -> V,
    {
        let mut inner = self.get_or_init_inner();
        let v = inner.entry(partition_key).or_insert_with(|| Arc::new(f()));
        v.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::KeyedPartition;
    use std::sync::Arc;

    #[test]
    fn test_keyed_partition_returns_same_value_for_same_key() {
        let kp = KeyedPartition::new();
        let _ = kp.get_or_init("A", || "A".to_owned());
        let actual = kp.get_or_init("A", || "B".to_owned());
        let expected = Arc::new("A".to_owned());
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_keyed_partition_returns_different_value_for_different_key() {
        let kp = KeyedPartition::new();
        let _ = kp.get_or_init("A", || "A".to_owned());
        let actual = kp.get_or_init("B", || "B".to_owned());

        let expected = Arc::new("B".to_owned());
        assert_eq!(expected, actual);

        let actual = kp.get("A").unwrap();
        let expected = Arc::new("A".to_owned());
        assert_eq!(expected, actual);
    }
}
