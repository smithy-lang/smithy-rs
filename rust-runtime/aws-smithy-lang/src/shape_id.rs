/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Absolute Smithy shape identifiers.

use std::borrow::Borrow;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::sync::Arc;

/// An absolute Smithy shape ID such as `com.example#Shape` or `com.example#Shape$member`.
///
/// `ShapeId` is immutable and cheap to clone. Equality, ordering, and hashing are
/// case-sensitive and use the complete absolute ID, and `ShapeId` implements
/// [`Borrow<str>`] so a `HashMap<ShapeId, _>` can be queried with a `&str`.
#[derive(Clone)]
pub struct ShapeId {
    absolute: Arc<str>,
    hash_index: u32,
    dollar_index: Option<u32>,
}

impl ShapeId {
    /// Parses an absolute shape ID.
    pub fn new(absolute: &str) -> Result<Self, InvalidShapeIdError> {
        Self::from_arc(Arc::from(absolute))
    }

    pub(crate) fn from_arc(absolute: Arc<str>) -> Result<Self, InvalidShapeIdError> {
        let err = || InvalidShapeIdError::new(&absolute);
        if absolute.len() > u32::MAX as usize {
            return Err(err());
        }
        let hash_index = absolute.find('#').ok_or_else(err)?;
        let (namespace, rest) = (&absolute[..hash_index], &absolute[hash_index + 1..]);
        if !is_valid_namespace(namespace) {
            return Err(err());
        }
        let dollar_index = match rest.find('$') {
            Some(i) => {
                if !is_valid_identifier(&rest[..i]) || !is_valid_identifier(&rest[i + 1..]) {
                    return Err(err());
                }
                Some((hash_index + 1 + i) as u32)
            }
            None if is_valid_identifier(rest) => None,
            None => return Err(err()),
        };
        Ok(Self {
            hash_index: hash_index as u32,
            dollar_index,
            absolute,
        })
    }

    /// Builds a shape ID from its namespace, name, and optional member name.
    pub fn from_parts(
        namespace: &str,
        name: &str,
        member: Option<&str>,
    ) -> Result<Self, InvalidShapeIdError> {
        let absolute = match member {
            Some(member) => format!("{namespace}#{name}${member}"),
            None => format!("{namespace}#{name}"),
        };
        Self::new(&absolute)
    }

    /// Parses `id`, resolving a relative ID (one without `#`) against `namespace`.
    ///
    /// Smithy JSON AST requires absolute IDs; this is for callers that hold relative names.
    pub fn from_relative(namespace: &str, id: &str) -> Result<Self, InvalidShapeIdError> {
        if id.contains('#') {
            Self::new(id)
        } else {
            Self::new(&format!("{namespace}#{id}"))
        }
    }

    /// The namespace, for example `com.example`.
    pub fn namespace(&self) -> &str {
        &self.absolute[..self.hash_index as usize]
    }

    /// The shape name, without namespace or member, for example `Shape`.
    pub fn name(&self) -> &str {
        let end = self
            .dollar_index
            .map_or(self.absolute.len(), |i| i as usize);
        &self.absolute[self.hash_index as usize + 1..end]
    }

    /// The member name, if this is a member ID.
    pub fn member(&self) -> Option<&str> {
        self.dollar_index.map(|i| &self.absolute[i as usize + 1..])
    }

    /// Returns `true` if this ID has a member component.
    pub fn is_member(&self) -> bool {
        self.dollar_index.is_some()
    }

    /// The complete absolute ID.
    pub fn as_str(&self) -> &str {
        &self.absolute
    }

    /// The root shape ID: this ID without its member component.
    pub fn root(&self) -> ShapeId {
        match self.dollar_index {
            None => self.clone(),
            Some(i) => Self {
                absolute: Arc::from(&self.absolute[..i as usize]),
                hash_index: self.hash_index,
                dollar_index: None,
            },
        }
    }

    /// Returns the ID of `member` within this ID's root shape.
    pub fn with_member(&self, member: &str) -> Result<ShapeId, InvalidShapeIdError> {
        if !is_valid_identifier(member) {
            return Err(InvalidShapeIdError::new(&format!(
                "{}${member}",
                self.root()
            )));
        }
        let root = self.root();
        let dollar_index = root.absolute.len() as u32;
        Ok(Self {
            absolute: Arc::from(format!("{}${member}", root.absolute)),
            hash_index: self.hash_index,
            dollar_index: Some(dollar_index),
        })
    }

    /// Returns `true` if `value` is a valid Smithy identifier.
    pub fn is_valid_identifier(value: &str) -> bool {
        is_valid_identifier(value)
    }

    /// Returns `true` if `value` is a valid Smithy namespace.
    pub fn is_valid_namespace(value: &str) -> bool {
        is_valid_namespace(value)
    }

    #[cfg(test)]
    pub(crate) fn arc(&self) -> &Arc<str> {
        &self.absolute
    }
}

/// `identifier = *"_" ALPHA *(ALPHA / DIGIT / "_")`
fn is_valid_identifier(value: &str) -> bool {
    let bytes = value.as_bytes();
    let start = bytes.iter().take_while(|b| **b == b'_').count();
    match bytes.get(start) {
        Some(b) if b.is_ascii_alphabetic() => bytes[start + 1..]
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'_'),
        _ => false,
    }
}

fn is_valid_namespace(value: &str) -> bool {
    value.split('.').all(is_valid_identifier)
}

impl PartialEq for ShapeId {
    fn eq(&self, other: &Self) -> bool {
        self.absolute == other.absolute
    }
}

impl Eq for ShapeId {}

impl PartialOrd for ShapeId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ShapeId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.absolute.cmp(&other.absolute)
    }
}

impl Hash for ShapeId {
    // Must agree with `str::hash` because of `Borrow<str>`.
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state)
    }
}

impl Borrow<str> for ShapeId {
    fn borrow(&self) -> &str {
        &self.absolute
    }
}

impl AsRef<str> for ShapeId {
    fn as_ref(&self) -> &str {
        &self.absolute
    }
}

impl PartialEq<str> for ShapeId {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for ShapeId {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl FromStr for ShapeId {
    type Err = InvalidShapeIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<&str> for ShapeId {
    type Error = InvalidShapeIdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl fmt::Display for ShapeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.absolute)
    }
}

impl fmt::Debug for ShapeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ShapeId({:?})", &*self.absolute)
    }
}

/// A string is not a valid absolute Smithy shape ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidShapeIdError {
    value: String,
}

impl InvalidShapeIdError {
    fn new(value: &str) -> Self {
        Self {
            value: value.to_owned(),
        }
    }

    /// The rejected input.
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl fmt::Display for InvalidShapeIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid absolute shape ID `{}`", self.value)
    }
}

impl std::error::Error for InvalidShapeIdError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn parses_parts() {
        let id = ShapeId::new("com.example#Shape$member").unwrap();
        assert_eq!(id.namespace(), "com.example");
        assert_eq!(id.name(), "Shape");
        assert_eq!(id.member(), Some("member"));
        assert_eq!(id.root().as_str(), "com.example#Shape");
        assert_eq!(
            id.root().with_member("other").unwrap(),
            "com.example#Shape$other"
        );
        let root = ShapeId::new("smithy.api#String").unwrap();
        assert_eq!(root.member(), None);
        assert!(!root.is_member());
    }

    #[test]
    fn grammar() {
        for ok in ["a#B", "__a.b_1#_C2", "a.b.c#D$e", "A#a$_x1"] {
            assert!(ShapeId::new(ok).is_ok(), "{ok}");
        }
        for bad in [
            "", "#A", "a#", "a#B$", "a#$b", "a.#B", ".a#B", "a..b#C", "1a#B", "a#1B", "a#_",
            "a#B$c$d", "a#B#C", "a#B c", "a#B$_1x", "a#B-c", "a#Bé", "a", "a#B$1",
        ] {
            assert!(ShapeId::new(bad).is_err(), "{bad}");
        }
        assert!(ShapeId::from_parts("a", "B", Some("c")).is_ok());
        assert!(ShapeId::from_parts("a", "B#", None).is_err());
        assert_eq!(ShapeId::from_relative("a.b", "C").unwrap(), "a.b#C");
        assert_eq!(ShapeId::from_relative("a.b", "x#C").unwrap(), "x#C");
    }

    #[test]
    fn equality_hash_and_borrow() {
        let a = ShapeId::new("a#B").unwrap();
        let b: ShapeId = String::from("a#B").parse().unwrap();
        assert_eq!(a, b);
        assert_ne!(a, ShapeId::new("a#b").unwrap());
        let mut map = HashMap::new();
        map.insert(a, 1);
        assert_eq!(map.get("a#B"), Some(&1));
        assert_eq!(map.get(&b), Some(&1));
        assert!(ShapeId::new("a#A").unwrap() < ShapeId::new("a#B").unwrap());
    }

    #[test]
    fn send_sync() {
        fn assert<T: Send + Sync + 'static>() {}
        assert::<ShapeId>();
        assert::<InvalidShapeIdError>();
    }
}
