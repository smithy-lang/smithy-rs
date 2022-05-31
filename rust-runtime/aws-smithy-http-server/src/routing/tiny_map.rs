/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{
    borrow::Borrow,
    collections::{hash_map::RandomState, HashMap},
    fmt,
    hash::{BuildHasher, Hash},
};

/// A map implementation with fast iteration which switches backing storage from [`Vec`] to
/// [`HashMap`] when the number of entries exceeds `CUTOFF` (which defaults to 20).
#[derive(Clone)]
pub struct TinyMap<K, V, S = RandomState, const CUTOFF: usize = 20> {
    inner: TinyMapInner<K, V, S, CUTOFF>,
}

impl<K, V, S, const CUTOFF: usize> fmt::Debug for TinyMap<K, V, S, CUTOFF>
where
    K: fmt::Debug,
    V: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TinyMap").field("inner", &self.inner).finish()
    }
}

#[derive(Clone)]
enum TinyMapInner<K, V, S, const CUTOFF: usize> {
    Vec(Vec<(K, V)>),
    HashMap(HashMap<K, V, S>),
}

impl<K, V, S, const CUTOFF: usize> fmt::Debug for TinyMapInner<K, V, S, CUTOFF>
where
    K: fmt::Debug,
    V: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Vec(arg0) => f.debug_tuple("Vec").field(arg0).finish(),
            Self::HashMap(arg0) => f.debug_tuple("HashMap").field(arg0).finish(),
        }
    }
}

enum EitherIterator<Left, Right> {
    Left(Left),
    Right(Right),
}

impl<Left, Right> Iterator for EitherIterator<Left, Right>
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
    inner: EitherIterator<std::vec::IntoIter<(K, V)>, std::collections::hash_map::IntoIter<K, V>>,
}

impl<K, V> Iterator for IntoIter<K, V> {
    type Item = (K, V);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

impl<K, V, S, const CUTOFF: usize> IntoIterator for TinyMap<K, V, S, CUTOFF> {
    type Item = (K, V);

    type IntoIter = IntoIter<K, V>;

    fn into_iter(self) -> Self::IntoIter {
        let inner = match self.inner {
            TinyMapInner::Vec(vec) => EitherIterator::Left(vec.into_iter()),
            TinyMapInner::HashMap(hash_map) => EitherIterator::Right(hash_map.into_iter()),
        };
        IntoIter { inner }
    }
}

impl<K, V, S, const CUTOFF: usize> FromIterator<(K, V)> for TinyMap<K, V, S, CUTOFF>
where
    K: Hash + Eq,
    S: BuildHasher + Default,
{
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self {
        let mut vec = Vec::with_capacity(CUTOFF);
        let mut iter = iter.into_iter().enumerate();

        // Populate the Vec
        while let Some((index, pair)) = iter.next() {
            // If overflow CUTOFF then return a HashMap instead
            if index == CUTOFF {
                let inner = TinyMapInner::HashMap(vec.into_iter().chain(iter.map(|(_, pair)| pair)).collect());
                return TinyMap { inner };
            }

            vec.push(pair);
        }

        TinyMap {
            inner: TinyMapInner::Vec(vec),
        }
    }
}

impl<K, V, S, const CUTOFF: usize> TinyMap<K, V, S, CUTOFF>
where
    K: Eq + Hash,
    S: BuildHasher,
{
    /// Returns a reference to the value corresponding to the key.
    ///
    /// The key may be borrowed form of map's key type, but [`Hash`] and [`Eq`] on the borrowed
    /// form _must_ match those for the key type.
    pub fn get<Q: ?Sized>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq,
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

    const SMALL_VALUES: [(&'static str, usize); 3] = [("a", 0), ("b", 1), ("c", 2)];
    const MEDIUM_VALUES: [(&'static str, usize); 5] = [("a", 0), ("b", 1), ("c", 2), ("d", 3), ("e", 4)];
    const LARGE_VALUES: [(&'static str, usize); 10] = [
        ("a", 0),
        ("b", 1),
        ("c", 2),
        ("d", 3),
        ("e", 4),
        ("f", 5),
        ("g", 6),
        ("h", 7),
        ("i", 8),
        ("k", 9),
    ];

    #[test]
    fn collect_small() {
        let tiny_map: TinyMap<_, _, RandomState, CUTOFF> = SMALL_VALUES.into_iter().collect();
        assert!(matches!(tiny_map.inner, TinyMapInner::Vec(_)))
    }

    #[test]
    fn collect_medium() {
        let tiny_map: TinyMap<_, _, RandomState, CUTOFF> = MEDIUM_VALUES.into_iter().collect();
        assert!(matches!(tiny_map.inner, TinyMapInner::Vec(_)))
    }

    #[test]
    fn collect_large() {
        let tiny_map: TinyMap<_, _, RandomState, CUTOFF> = LARGE_VALUES.into_iter().collect();
        assert!(matches!(tiny_map.inner, TinyMapInner::HashMap(_)))
    }

    #[test]
    fn get_small_success() {
        let tiny_map: TinyMap<_, _, RandomState, CUTOFF> = SMALL_VALUES.into_iter().collect();
        assert_eq!(tiny_map.get("a"), Some(&0))
    }

    #[test]
    fn get_medium_success() {
        let tiny_map: TinyMap<_, _, RandomState, CUTOFF> = MEDIUM_VALUES.into_iter().collect();
        assert_eq!(tiny_map.get("d"), Some(&3))
    }

    #[test]
    fn get_large_success() {
        let tiny_map: TinyMap<_, _, RandomState, CUTOFF> = LARGE_VALUES.into_iter().collect();
        assert_eq!(tiny_map.get("h"), Some(&7))
    }

    #[test]
    fn get_small_fail() {
        let tiny_map: TinyMap<_, _, RandomState, CUTOFF> = SMALL_VALUES.into_iter().collect();
        assert_eq!(tiny_map.get("x"), None)
    }

    #[test]
    fn get_medium_fail() {
        let tiny_map: TinyMap<_, _, RandomState, CUTOFF> = MEDIUM_VALUES.into_iter().collect();
        assert_eq!(tiny_map.get("y"), None)
    }

    #[test]
    fn get_large_fail() {
        let tiny_map: TinyMap<_, _, RandomState, CUTOFF> = LARGE_VALUES.into_iter().collect();
        assert_eq!(tiny_map.get("z"), None)
    }
}
