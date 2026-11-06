/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! [`DocumentObject`], the insertion-ordered map inside [`DocumentV2::Object`].
//!
//! The SEP requires a document created from serialized data to iterate its map entries in the
//! order they appeared in that data. [`std::collections::HashMap`], which
//! [`Document::Object`](crate::Document::Object) wraps, iterates in a randomized order, so it
//! can't meet that requirement.
//!
//! `DocumentObject` offers the familiar map methods (`insert`, `get`, `contains_key`, `len`,
//! `iter`, `Index<&str>`, and so on) and iterates in insertion order. Its storage is private,
//! so it can change without breaking callers.
//!
//! # Example
//!
//! ```
//! use aws_smithy_types::{DocumentV2, Number, document::DocumentObject};
//!
//! let mut object = DocumentObject::new();
//! object.insert("greeting".to_string(), DocumentV2::String("hello".to_string()));
//! object.insert("count".to_string(), DocumentV2::Number(Number::PosInt(42)));
//!
//! let document = DocumentV2::Object(object);
//! let object = document.as_object().unwrap();
//!
//! assert_eq!(object.len(), 2);
//! assert!(object.contains_key("greeting"));
//!
//! // Iteration order matches insertion order.
//! let keys: Vec<&str> = object.keys().map(|k| k.as_str()).collect();
//! assert_eq!(keys, ["greeting", "count"]);
//! ```

use super::DocumentV2;
use std::ops::Index;

/// An insertion-ordered map from string keys to [`DocumentV2`] values.
///
/// See the [module documentation](self) for context.
///
/// Two objects are equal when they hold the same entries, in any order, as two `HashMap`s
/// would be.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DocumentObject {
    inner: indexmap::IndexMap<String, DocumentV2>,
}

impl DocumentObject {
    /// Creates an empty `DocumentObject`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an empty `DocumentObject` with at least the specified capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: indexmap::IndexMap::with_capacity(capacity),
        }
    }

    /// Returns the number of entries.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns `true` if the object contains no entries.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Removes all entries.
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// Inserts a key-value pair.
    ///
    /// If the key is already present, its value is replaced at the key's existing position and
    /// the old value is returned. Otherwise the entry is added at the end and `None` is
    /// returned.
    pub fn insert(&mut self, key: String, value: DocumentV2) -> Option<DocumentV2> {
        self.inner.insert(key, value)
    }

    /// Returns a reference to the value for `key`, if present.
    pub fn get(&self, key: &str) -> Option<&DocumentV2> {
        self.inner.get(key)
    }

    /// Returns a mutable reference to the value for `key`, if present.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut DocumentV2> {
        self.inner.get_mut(key)
    }

    /// Returns `true` if the object contains an entry for `key`.
    pub fn contains_key(&self, key: &str) -> bool {
        self.inner.contains_key(key)
    }

    /// Removes the entry for `key` and returns its value, or `None` if there was no entry.
    ///
    /// The remaining entries keep their order.
    pub fn remove(&mut self, key: &str) -> Option<DocumentV2> {
        self.inner.shift_remove(key)
    }

    /// Returns an iterator over the entries in insertion order.
    pub fn iter(&self) -> Iter<'_> {
        Iter {
            inner: self.inner.iter(),
        }
    }

    /// Returns an iterator over the entries in insertion order, with mutable values.
    pub fn iter_mut(&mut self) -> IterMut<'_> {
        IterMut {
            inner: self.inner.iter_mut(),
        }
    }

    /// Returns an iterator over the keys in insertion order.
    pub fn keys(&self) -> Keys<'_> {
        Keys {
            inner: self.inner.keys(),
        }
    }

    /// Returns an iterator over the values in insertion order.
    pub fn values(&self) -> Values<'_> {
        Values {
            inner: self.inner.values(),
        }
    }

    /// Returns an iterator over mutable values in insertion order.
    pub fn values_mut(&mut self) -> ValuesMut<'_> {
        ValuesMut {
            inner: self.inner.values_mut(),
        }
    }
}

// These wrappers keep `indexmap`'s iterator types out of the public API.

/// An iterator over the entries of a [`DocumentObject`].
#[derive(Debug)]
pub struct Iter<'a> {
    inner: indexmap::map::Iter<'a, String, DocumentV2>,
}

impl<'a> Iterator for Iter<'a> {
    type Item = (&'a String, &'a DocumentV2);
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl ExactSizeIterator for Iter<'_> {
    fn len(&self) -> usize {
        self.inner.len()
    }
}

/// An iterator over the entries of a [`DocumentObject`], with mutable values.
#[derive(Debug)]
pub struct IterMut<'a> {
    inner: indexmap::map::IterMut<'a, String, DocumentV2>,
}

impl<'a> Iterator for IterMut<'a> {
    type Item = (&'a String, &'a mut DocumentV2);
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl ExactSizeIterator for IterMut<'_> {
    fn len(&self) -> usize {
        self.inner.len()
    }
}

/// An iterator over the keys of a [`DocumentObject`].
#[derive(Debug)]
pub struct Keys<'a> {
    inner: indexmap::map::Keys<'a, String, DocumentV2>,
}

impl<'a> Iterator for Keys<'a> {
    type Item = &'a String;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl ExactSizeIterator for Keys<'_> {
    fn len(&self) -> usize {
        self.inner.len()
    }
}

/// An iterator over the values of a [`DocumentObject`].
#[derive(Debug)]
pub struct Values<'a> {
    inner: indexmap::map::Values<'a, String, DocumentV2>,
}

impl<'a> Iterator for Values<'a> {
    type Item = &'a DocumentV2;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl ExactSizeIterator for Values<'_> {
    fn len(&self) -> usize {
        self.inner.len()
    }
}

/// An iterator over the mutable values of a [`DocumentObject`].
#[derive(Debug)]
pub struct ValuesMut<'a> {
    inner: indexmap::map::ValuesMut<'a, String, DocumentV2>,
}

impl<'a> Iterator for ValuesMut<'a> {
    type Item = &'a mut DocumentV2;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl ExactSizeIterator for ValuesMut<'_> {
    fn len(&self) -> usize {
        self.inner.len()
    }
}

/// An owning iterator over the entries of a [`DocumentObject`].
#[derive(Debug)]
pub struct IntoIter {
    inner: indexmap::map::IntoIter<String, DocumentV2>,
}

impl Iterator for IntoIter {
    type Item = (String, DocumentV2);
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl ExactSizeIterator for IntoIter {
    fn len(&self) -> usize {
        self.inner.len()
    }
}

impl IntoIterator for DocumentObject {
    type Item = (String, DocumentV2);
    type IntoIter = IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        IntoIter {
            inner: self.inner.into_iter(),
        }
    }
}

impl<'a> IntoIterator for &'a DocumentObject {
    type Item = (&'a String, &'a DocumentV2);
    type IntoIter = Iter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> IntoIterator for &'a mut DocumentObject {
    type Item = (&'a String, &'a mut DocumentV2);
    type IntoIter = IterMut<'a>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

/// Panics if the key isn't present, like `HashMap`'s `Index` implementation.
impl Index<&str> for DocumentObject {
    type Output = DocumentV2;
    fn index(&self, key: &str) -> &DocumentV2 {
        self.get(key)
            .unwrap_or_else(|| panic!("DocumentObject: no entry for key {key:?}"))
    }
}

/// Panics if the key isn't present, like `HashMap`'s `Index` implementation.
impl Index<&String> for DocumentObject {
    type Output = DocumentV2;
    fn index(&self, key: &String) -> &DocumentV2 {
        &self[key.as_str()]
    }
}

impl FromIterator<(String, DocumentV2)> for DocumentObject {
    fn from_iter<I: IntoIterator<Item = (String, DocumentV2)>>(iter: I) -> Self {
        Self {
            inner: indexmap::IndexMap::from_iter(iter),
        }
    }
}

impl Extend<(String, DocumentV2)> for DocumentObject {
    fn extend<I: IntoIterator<Item = (String, DocumentV2)>>(&mut self, iter: I) {
        self.inner.extend(iter);
    }
}

impl<const N: usize> From<[(String, DocumentV2); N]> for DocumentObject {
    fn from(entries: [(String, DocumentV2); N]) -> Self {
        Self {
            inner: indexmap::IndexMap::from(entries),
        }
    }
}

impl From<std::collections::HashMap<String, DocumentV2>> for DocumentObject {
    /// Converts a `HashMap` to a `DocumentObject`.
    ///
    /// The object's iteration order is the `HashMap`'s iteration order, which is unspecified.
    /// To control the order, build the object with `insert` instead.
    fn from(map: std::collections::HashMap<String, DocumentV2>) -> Self {
        map.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(object: &DocumentObject) -> Vec<&str> {
        object.keys().map(String::as_str).collect()
    }

    #[test]
    fn iteration_follows_insertion_order() {
        let mut object = DocumentObject::new();
        object.insert("zebra".to_owned(), DocumentV2::Bool(true));
        object.insert("alpha".to_owned(), DocumentV2::Bool(false));
        object.insert("middle".to_owned(), DocumentV2::Null);

        assert_eq!(keys(&object), ["zebra", "alpha", "middle"]);
        let values: Vec<&DocumentV2> = object.values().collect();
        assert_eq!(
            values,
            [
                &DocumentV2::Bool(true),
                &DocumentV2::Bool(false),
                &DocumentV2::Null
            ]
        );
        let owned: Vec<String> = object.into_iter().map(|(key, _)| key).collect();
        assert_eq!(owned, ["zebra", "alpha", "middle"]);
    }

    #[test]
    fn replacing_a_value_keeps_the_key_position() {
        let mut object = DocumentObject::from([
            ("first".to_owned(), DocumentV2::Bool(false)),
            ("second".to_owned(), DocumentV2::Bool(false)),
            ("third".to_owned(), DocumentV2::Bool(false)),
        ]);

        let previous = object.insert("second".to_owned(), DocumentV2::Bool(true));
        assert_eq!(previous, Some(DocumentV2::Bool(false)));
        assert_eq!(keys(&object), ["first", "second", "third"]);
        assert_eq!(object["second"], DocumentV2::Bool(true));
    }

    #[test]
    fn removing_an_entry_keeps_the_remaining_order() {
        let mut object = DocumentObject::from([
            ("first".to_owned(), DocumentV2::Null),
            ("second".to_owned(), DocumentV2::Null),
            ("third".to_owned(), DocumentV2::Null),
        ]);

        assert_eq!(object.remove("second"), Some(DocumentV2::Null));
        assert_eq!(object.remove("second"), None);
        assert_eq!(keys(&object), ["first", "third"]);
    }

    #[test]
    fn equality_ignores_order() {
        let forward = DocumentObject::from([
            ("a".to_owned(), DocumentV2::Null),
            ("b".to_owned(), DocumentV2::Bool(true)),
        ]);
        let backward = DocumentObject::from([
            ("b".to_owned(), DocumentV2::Bool(true)),
            ("a".to_owned(), DocumentV2::Null),
        ]);
        assert_eq!(forward, backward);
        assert_ne!(forward, DocumentObject::new());
    }

    #[test]
    fn mutation_through_iterators_and_get_mut() {
        let mut object = DocumentObject::from([
            ("a".to_owned(), DocumentV2::Null),
            ("b".to_owned(), DocumentV2::Null),
        ]);
        for (_, value) in object.iter_mut() {
            *value = DocumentV2::Bool(true);
        }
        *object.get_mut("b").unwrap() = DocumentV2::Bool(false);
        for value in object.values_mut() {
            if value.is_null() {
                *value = DocumentV2::Bool(false);
            }
        }
        assert_eq!(object["a"], DocumentV2::Bool(true));
        assert_eq!(object[&"b".to_owned()], DocumentV2::Bool(false));
    }

    #[test]
    fn collecting_and_extending_preserve_order() {
        let mut object: DocumentObject = [("z".to_owned(), DocumentV2::Null)].into_iter().collect();
        object.extend([("a".to_owned(), DocumentV2::Null)]);
        assert_eq!(keys(&object), ["z", "a"]);
        assert_eq!(object.iter().len(), 2);
        object.clear();
        assert!(object.is_empty());
    }

    #[test]
    #[should_panic(expected = "no entry for key \"missing\"")]
    fn index_panics_on_a_missing_key() {
        let _ = &DocumentObject::new()["missing"];
    }
}
