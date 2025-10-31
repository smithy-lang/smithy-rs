/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{borrow::Borrow, collections::HashMap, hash::Hash};

/// A map implementation with fast iteration which switches backing storage from [`Vec`] to
/// [`HashMap`] when the number of entries exceeds `CUTOFF`.
#[derive(Clone, Debug)]
pub struct TinyMap<K, V, const CUTOFF: usize> {
    inner: TinyMapInner<K, V, CUTOFF>,
}

#[derive(Clone, Debug)]
enum TinyMapInner<K, V, const CUTOFF: usize> {
    Vec(Vec<(K, V)>),
    HashMap(HashMap<K, V>),
}

enum OrIterator<Left, Right> {
    Left(Left),
    Right(Right),
}

impl<Left, Right> Iterator for OrIterator<Left, Right>
where
    Left: Iterator,
    Right: Iterator<Item = Left::Item>,
{
    type Item = Left::Item;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Left(left) => left.next(),
            Self::Right(right) => right.next(),
        }
    }
}

/// An owning iterator over the entries of a `TinyMap`.
///
/// This struct is created by the [`into_iter`](IntoIterator::into_iter) method on [`TinyMap`] (
/// provided by the [`IntoIterator`] trait). See its documentation for more.
pub struct IntoIter<K, V> {
    inner: OrIterator<std::vec::IntoIter<(K, V)>, std::collections::hash_map::IntoIter<K, V>>,
}

impl<K, V> Iterator for IntoIter<K, V> {
    type Item = (K, V);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

impl<K, V, const CUTOFF: usize> IntoIterator for TinyMap<K, V, CUTOFF> {
    type Item = (K, V);

    type IntoIter = IntoIter<K, V>;

    fn into_iter(self) -> Self::IntoIter {
        let inner = match self.inner {
            TinyMapInner::Vec(vec) => OrIterator::Left(vec.into_iter()),
            TinyMapInner::HashMap(hash_map) => OrIterator::Right(hash_map.into_iter()),
        };
        IntoIter { inner }
    }
}

impl<K, V, const CUTOFF: usize> FromIterator<(K, V)> for TinyMap<K, V, CUTOFF>
where
    K: Hash + Eq,
{
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self {
        let mut vec = Vec::with_capacity(CUTOFF);
        let mut iter = iter.into_iter().enumerate();

        // Populate the `Vec`
        while let Some((index, pair)) = iter.next() {
            vec.push(pair);

            // If overflow `CUTOFF` then return a `HashMap` instead
            if index == CUTOFF {
                let inner = TinyMapInner::HashMap(vec.into_iter().chain(iter.map(|(_, pair)| pair)).collect());
                return TinyMap { inner };
            }
        }

        TinyMap {
            inner: TinyMapInner::Vec(vec),
        }
    }
}

impl<K, V, const CUTOFF: usize> TinyMap<K, V, CUTOFF>
where
    K: Eq + Hash,
{
    /// Returns a reference to the value corresponding to the key.
    ///
    /// The key may be borrowed form of map's key type, but [`Hash`] and [`Eq`] on the borrowed
    /// form _must_ match those for the key type.
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        match &self.inner {
            TinyMapInner::Vec(vec) => vec
                .iter()
                .find(|(key_inner, _)| key_inner.borrow() == key)
                .map(|(_, value)| value),
            TinyMapInner::HashMap(hash_map) => hash_map.get(key),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUTOFF: usize = 5;

    const SMALL_VALUES: [(&str, usize); 3] = [("a", 0), ("b", 1), ("c", 2)];
    const MEDIUM_VALUES: [(&str, usize); 5] = [("a", 0), ("b", 1), ("c", 2), ("d", 3), ("e", 4)];
    const LARGE_VALUES: [(&str, usize); 10] = [
        ("a", 0),
        ("b", 1),
        ("c", 2),
        ("d", 3),
        ("e", 4),
        ("f", 5),
        ("g", 6),
        ("h", 7),
        ("i", 8),
        ("j", 9),
    ];

    #[test]
    fn collect_small() {
        let tiny_map: TinyMap<_, _, CUTOFF> = SMALL_VALUES.into_iter().collect();
        assert!(matches!(tiny_map.inner, TinyMapInner::Vec(_)))
    }

    #[test]
    fn collect_medium() {
        let tiny_map: TinyMap<_, _, CUTOFF> = MEDIUM_VALUES.into_iter().collect();
        assert!(matches!(tiny_map.inner, TinyMapInner::Vec(_)))
    }

    #[test]
    fn collect_large() {
        let tiny_map: TinyMap<_, _, CUTOFF> = LARGE_VALUES.into_iter().collect();
        assert!(matches!(tiny_map.inner, TinyMapInner::HashMap(_)))
    }

    #[test]
    fn get_small_success() {
        let tiny_map: TinyMap<_, _, CUTOFF> = SMALL_VALUES.into_iter().collect();
        SMALL_VALUES.into_iter().for_each(|(op, val)| {
            assert_eq!(tiny_map.get(op), Some(&val));
        });
    }

    #[test]
    fn get_medium_success() {
        let tiny_map: TinyMap<_, _, CUTOFF> = MEDIUM_VALUES.into_iter().collect();
        MEDIUM_VALUES.into_iter().for_each(|(op, val)| {
            assert_eq!(tiny_map.get(op), Some(&val));
        });
    }

    #[test]
    fn get_large_success() {
        let tiny_map: TinyMap<_, _, CUTOFF> = LARGE_VALUES.into_iter().collect();
        LARGE_VALUES.into_iter().for_each(|(op, val)| {
            assert_eq!(tiny_map.get(op), Some(&val));
        });
    }

    #[test]
    fn get_small_fail() {
        let tiny_map: TinyMap<_, _, CUTOFF> = SMALL_VALUES.into_iter().collect();
        assert_eq!(tiny_map.get("x"), None)
    }

    #[test]
    fn get_medium_fail() {
        let tiny_map: TinyMap<_, _, CUTOFF> = MEDIUM_VALUES.into_iter().collect();
        assert_eq!(tiny_map.get("y"), None)
    }

    #[test]
    fn get_large_fail() {
        let tiny_map: TinyMap<_, _, CUTOFF> = LARGE_VALUES.into_iter().collect();
        assert_eq!(tiny_map.get("z"), None)
    }
}
