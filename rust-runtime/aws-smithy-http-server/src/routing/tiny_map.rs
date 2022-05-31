use std::{
    borrow::Borrow,
    collections::{hash_map::RandomState, HashMap},
    hash::{BuildHasher, Hash},
};

const SIZE: usize = 20;

#[derive(Debug, Clone)]
pub struct TinyMap<K, V, S = RandomState> {
    inner: TinyMapInner<K, V, S>,
}

#[derive(Debug, Clone)]
enum TinyMapInner<K, V, S> {
    Vec(Vec<(K, V)>),
    HashMap(HashMap<K, V, S>),
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

pub struct IntoIter<K, V> {
    inner: EitherIterator<std::vec::IntoIter<(K, V)>, std::collections::hash_map::IntoIter<K, V>>,
}

impl<K, V> Iterator for IntoIter<K, V> {
    type Item = (K, V);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

impl<K, V, S> IntoIterator for TinyMap<K, V, S> {
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

impl<K, V, S> FromIterator<(K, V)> for TinyMap<K, V, S>
where
    K: Hash + Eq,
    S: BuildHasher + Default,
{
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self {
        let mut vec = Vec::with_capacity(SIZE);
        let mut iter = iter.into_iter().enumerate();

        // Populate the Vec
        while let Some((index, pair)) = iter.next() {
            // If overflow SIZE then return a HashMap instead
            if index == SIZE {
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

impl<K, V, S> TinyMap<K, V, S>
where
    K: Eq + Hash,
    S: BuildHasher,
{
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
