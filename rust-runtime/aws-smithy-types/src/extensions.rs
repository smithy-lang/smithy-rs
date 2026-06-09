/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! A type-erased map of extensions, keyed by type.
//!
//! [`Extensions`] stores at most one value of any given type, retrieved by that
//! type. It is used to carry auxiliary data that is derived during request
//! processing but is not part of a modeled shape, for example the outcome of
//! response checksum validation or per-request telemetry.

use crate::type_erasure::TypeErasedBox;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;
use std::hash::{BuildHasherDefault, Hasher};

/// A hasher for `TypeId`s that does no work.
///
/// A `TypeId` is already a well-distributed hash produced by the compiler, so
/// the map can use its `u64` directly instead of hashing it again.
#[derive(Default)]
struct IdHasher(u64);

impl Hasher for IdHasher {
    fn write(&mut self, _: &[u8]) {
        unreachable!("TypeId calls write_u64");
    }

    #[inline]
    fn write_u64(&mut self, id: u64) {
        self.0 = id;
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

type AnyMap = HashMap<TypeId, TypeErasedBox, BuildHasherDefault<IdHasher>>;

/// A type-erased map of extensions, keyed by type.
///
/// Holds at most one value of any given type. Inserting a second value of the
/// same type replaces the first. Stored values must be `Clone` so that
/// `Extensions` itself is `Clone`.
///
/// ```
/// use aws_smithy_types::extensions::Extensions;
///
/// #[derive(Clone, Debug, PartialEq)]
/// struct Marker(u32);
///
/// let mut ext = Extensions::new();
/// assert_eq!(ext.insert(Marker(1)), None);
/// assert_eq!(ext.get::<Marker>(), Some(&Marker(1)));
/// // Inserting the same type again returns the previous value.
/// assert_eq!(ext.insert(Marker(2)), Some(Marker(1)));
/// assert_eq!(ext.get::<Marker>(), Some(&Marker(2)));
/// ```
// An empty `HashMap` is three words; boxing keeps an unused `Extensions` to a
// single word, since most values never carry any extensions.
#[derive(Default)]
pub struct Extensions {
    map: Option<Box<AnyMap>>,
}

impl Clone for Extensions {
    fn clone(&self) -> Self {
        // Every value is inserted through `insert`, which requires `Clone` and
        // stores a cloneable `TypeErasedBox`, so `try_clone` always succeeds.
        let map = self.map.as_ref().map(|m| {
            let cloned: AnyMap = m
                .iter()
                .map(|(k, v)| {
                    (
                        *k,
                        v.try_clone()
                            .expect("values are cloneable via Extensions::insert"),
                    )
                })
                .collect();
            Box::new(cloned)
        });
        Self { map }
    }
}

impl Extensions {
    /// Create an empty [`Extensions`].
    pub fn new() -> Self {
        Self { map: None }
    }

    /// Insert a value into the map, returning the previous value of the same
    /// type if one was present.
    pub fn insert<T: Any + Clone + fmt::Debug + Send + Sync + 'static>(
        &mut self,
        value: T,
    ) -> Option<T> {
        self.map
            .get_or_insert_with(Box::default)
            .insert(TypeId::of::<T>(), TypeErasedBox::new_with_clone(value))
            .and_then(|prev| prev.downcast::<T>().ok().map(|b| *b))
    }

    /// Get a reference to the value of type `T`, if present.
    pub fn get<T: Any + fmt::Debug + Send + Sync + 'static>(&self) -> Option<&T> {
        self.map
            .as_ref()?
            .get(&TypeId::of::<T>())
            .and_then(|b| b.downcast_ref())
    }

    /// Get a mutable reference to the value of type `T`, if present.
    pub fn get_mut<T: Any + fmt::Debug + Send + Sync + 'static>(&mut self) -> Option<&mut T> {
        self.map
            .as_mut()?
            .get_mut(&TypeId::of::<T>())
            .and_then(|b| b.downcast_mut())
    }

    /// Remove the value of type `T`, returning it if present.
    pub fn remove<T: Any + fmt::Debug + Send + Sync + 'static>(&mut self) -> Option<T> {
        self.map
            .as_mut()?
            .remove(&TypeId::of::<T>())
            .and_then(|b| b.downcast::<T>().ok().map(|b| *b))
    }

    /// Returns `true` if no extensions are present.
    pub fn is_empty(&self) -> bool {
        self.map.as_ref().is_none_or(|m| m.is_empty())
    }

    /// Returns the number of extensions present.
    pub fn len(&self) -> usize {
        self.map.as_ref().map_or(0, |m| m.len())
    }
}

impl fmt::Debug for Extensions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Extensions")
            .field("len", &self.len())
            .finish()
    }
}

/// Provides access to the [`Extensions`] carried by a type.
///
/// Implemented by generated operation outputs so callers can read auxiliary
/// data attached during request processing (for example response checksum
/// validation results) without it being part of a modeled shape.
pub trait ProvideExtensions {
    /// Returns the extensions attached to this value.
    fn extensions(&self) -> &Extensions;
}

#[cfg(test)]
mod tests {
    use super::Extensions;

    #[derive(Clone, Debug, PartialEq)]
    struct A(u32);
    #[derive(Clone, Debug, PartialEq)]
    struct B(String);

    #[test]
    fn insert_get_remove() {
        let mut ext = Extensions::new();
        assert!(ext.is_empty());

        assert_eq!(ext.insert(A(1)), None);
        assert_eq!(ext.insert(B("x".to_string())), None);
        assert_eq!(ext.len(), 2);

        assert_eq!(ext.get::<A>(), Some(&A(1)));
        assert_eq!(ext.get::<B>(), Some(&B("x".to_string())));

        // Replacing returns the prior value of that type.
        assert_eq!(ext.insert(A(2)), Some(A(1)));
        assert_eq!(ext.get::<A>(), Some(&A(2)));
        assert_eq!(ext.len(), 2);

        assert_eq!(ext.remove::<A>(), Some(A(2)));
        assert_eq!(ext.get::<A>(), None);
        assert_eq!(ext.len(), 1);
    }

    #[test]
    fn get_mut() {
        let mut ext = Extensions::new();
        ext.insert(A(1));
        if let Some(a) = ext.get_mut::<A>() {
            a.0 = 9;
        }
        assert_eq!(ext.get::<A>(), Some(&A(9)));
    }

    #[test]
    fn missing_type_is_none() {
        let ext = Extensions::new();
        assert_eq!(ext.get::<A>(), None);
    }

    #[test]
    fn clone_is_independent() {
        let mut ext = Extensions::new();
        ext.insert(A(1));
        let mut cloned = ext.clone();
        cloned.insert(A(2));
        assert_eq!(ext.get::<A>(), Some(&A(1)));
        assert_eq!(cloned.get::<A>(), Some(&A(2)));
    }
}
