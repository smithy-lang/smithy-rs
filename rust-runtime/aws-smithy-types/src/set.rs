/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use std::{borrow::Borrow, hash::Hash};

use indexmap::IndexSet;

/// A collection of ordered, non-repeating elements.
#[derive(Debug, Clone)]
pub struct Set<T> {
    inner: IndexSet<T>,
}

impl<T> Default for Set<T> {
    fn default() -> Self {
        Self {
            inner: Default::default(),
        }
    }
}

impl<T> PartialEq<Self> for Set<T>
where
    T: Eq + Hash,
{
    fn eq(&self, other: &Self) -> bool {
        self.inner.eq(&other.inner)
    }
}

impl<T> Eq for Set<T> where T: Eq + Hash {}

/// An owning iterator over the items of a `Set`.
///
/// This `struct` is created by the [`into_iter`] method on [`Set`]
/// (provided by the [`IntoIterator`] trait). See its documentation for more.
///
/// [`into_iter`]: IntoIterator::into_iter
/// [`IntoIterator`]: std::iter::IntoIterator
///
/// # Examples
///
/// ```
/// use aws_smithy_types::Set;
///
/// let a = Set::from([1, 2, 3]);
///
/// let mut iter = a.into_iter();
/// ```
#[derive(Debug)]
pub struct IntoIter<T> {
    inner: indexmap::set::IntoIter<T>,
}

// Methods additional to [`Iterator::next`] are manually implemented to mirror the manual
// implementations given in
// https://github.com/bluss/indexmap/blob/2109261aee09f69f8a703b7324d86b44873d1de4/src/macros.rs#L77-L110
macro_rules! iter_shortcuts_impl {
    () => {
        fn next(&mut self) -> Option<Self::Item> {
            self.inner.next()
        }

        fn size_hint(&self) -> (usize, Option<usize>) {
            self.inner.size_hint()
        }

        fn count(self) -> usize
        where
            Self: Sized,
        {
            self.inner.count()
        }

        fn last(self) -> Option<Self::Item>
        where
            Self: Sized,
        {
            self.inner.last()
        }
    };
}

impl<T> Iterator for IntoIter<T> {
    type Item = T;

    iter_shortcuts_impl! {}
}

/// An iterator over the items of a `Set`.
///
/// This `struct` is created by the [`iter`] method on [`Set`].
/// See its documentation for more.
///
/// [`iter`]: Set::iter
///
/// # Examples
///
/// ```
/// use aws_smithy_types::Set;
///
/// let a = Set::from([1, 2, 3]);
///
/// let mut iter = a.iter();
/// ```
#[derive(Debug)]
pub struct Iter<'a, T> {
    inner: indexmap::set::Iter<'a, T>,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;

    iter_shortcuts_impl! {}
}

impl<T> Set<T> {
    /// Creates an empty `Set`.
    ///
    /// The hash set is initially created with a capacity of 0, so it will not allocate until it
    /// is first inserted into.
    ///
    /// # Examples
    ///
    /// ```
    /// use aws_smithy_types::Set;
    /// let set: Set<i32> = Set::new();
    /// ```
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// An iterator visiting all elements in insertion order.
    /// The iterator element type is `&'a T`.
    ///
    /// # Examples
    ///
    /// ```
    /// use aws_smithy_types::Set;
    /// let mut set = Set::new();
    /// set.insert("a");
    /// set.insert("b");
    ///
    /// // Will print in insertion order.
    /// for x in set.iter() {
    ///     println!("{x}");
    /// }
    /// ```
    #[inline]
    pub fn iter(&self) -> Iter<'_, T> {
        Iter {
            inner: self.inner.iter(),
        }
    }

    /// Returns the number of elements in the set.
    ///
    /// # Examples
    ///
    /// ```
    /// use aws_smithy_types::Set;
    ///
    /// let mut v = Set::new();
    /// assert_eq!(v.len(), 0);
    /// v.insert(1);
    /// assert_eq!(v.len(), 1);
    /// ```
    #[inline]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns `true` if the set contains no elements.
    ///
    /// # Examples
    ///
    /// ```
    /// use aws_smithy_types::Set;
    ///
    /// let mut v = Set::new();
    /// assert!(v.is_empty());
    /// v.insert(1);
    /// assert!(!v.is_empty());
    /// ```
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<T> Set<T>
where
    T: Eq + Hash,
{
    /// Adds a value to the set.
    ///
    /// If the set did not have this value present, `true` is returned.
    ///
    /// If the set did have this value present, `false` is returned.
    ///
    /// # Examples
    ///
    /// ```
    /// use aws_smithy_types::Set;
    ///
    /// let mut set = Set::new();
    ///
    /// assert_eq!(set.insert(2), true);
    /// assert_eq!(set.insert(2), false);
    /// assert_eq!(set.len(), 1);
    /// ```
    #[inline]
    pub fn insert(&mut self, value: T) -> bool {
        self.inner.insert(value)
    }

    /// This is an alias for [`Set::insert`]. This has been added to make the API across this and
    /// [`Vec`].
    #[doc(hidden)]
    #[inline]
    pub fn push(&mut self, value: T) -> bool {
        self.insert(value)
    }

    /// Returns `true` if the set contains a value.
    ///
    /// The value may be any borrowed form of the set's value type, but
    /// [`Hash`] and [`Eq`] on the borrowed form *must* match those for
    /// the value type.
    ///
    /// # Examples
    ///
    /// ```
    /// use aws_smithy_types::Set;
    ///
    /// let set = Set::from([1, 2, 3]);
    /// assert_eq!(set.contains(&1), true);
    /// assert_eq!(set.contains(&4), false);
    /// ```
    #[inline]
    pub fn contains<Q>(&self, value: &Q) -> bool
    where
        T: Borrow<Q>,
        Q: Hash + Eq,
    {
        self.inner.contains(value)
    }
}

impl<A> IntoIterator for Set<A> {
    type Item = A;

    type IntoIter = IntoIter<A>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        IntoIter {
            inner: self.inner.into_iter(),
        }
    }
}

impl<'a, A> IntoIterator for &'a Set<A> {
    type Item = &'a A;

    type IntoIter = Iter<'a, A>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<A> FromIterator<A> for Set<A>
where
    A: Eq + Hash,
{
    #[inline]
    fn from_iter<T: IntoIterator<Item = A>>(iter: T) -> Self {
        Self {
            inner: IndexSet::from_iter(iter),
        }
    }
}

impl<T, const N: usize> From<[T; N]> for Set<T>
where
    T: Eq + Hash,
{
    fn from(arr: [T; N]) -> Self {
        Self::from_iter(arr)
    }
}

impl<T> Extend<T> for Set<T>
where
    T: Eq + Hash,
{
    #[inline]
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        self.inner.extend(iter)
    }
}

impl<'a, T> Extend<&'a T> for Set<T>
where
    T: Eq + Hash + Copy + 'a,
{
    #[inline]
    fn extend<I: IntoIterator<Item = &'a T>>(&mut self, iter: I) {
        self.inner.extend(iter.into_iter().copied())
    }
}
