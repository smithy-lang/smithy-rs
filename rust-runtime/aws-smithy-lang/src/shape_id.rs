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
///
/// Ordering compares the absolute IDs byte by byte, matching `str`. (Java Smithy compares
/// case-insensitively first, so IDs that differ only in case can sort differently. A valid
/// model cannot contain such IDs.)
///
/// # Representation
///
/// A shape ID stores its absolute form once, either as a `&'static str` (see
/// [`from_static`](Self::from_static)) or as a shared `Arc<str>`, plus the offsets of `#` and
/// `$`. [`namespace`](Self::namespace), [`name`](Self::name), [`member`](Self::member), and
/// [`as_str`](Self::as_str) return borrowed slices of that string and never allocate. Cloning
/// increments a reference count (or copies a pointer for static IDs).
///
/// Java Smithy builds the absolute string of an ID created from parts lazily. This type builds
/// it eagerly instead: `Borrow<str>` requires a `ShapeId` to hash exactly like its absolute
/// string, so map lookups need that string anyway, and every ID parsed from a model already has
/// it. Building from parts costs one allocation for the absolute string.
#[derive(Clone)]
pub struct ShapeId {
    repr: Repr,
    hash_index: u32,
    /// The offset of `$`, or [`NO_MEMBER`].
    dollar_index: u32,
}

#[derive(Clone)]
enum Repr {
    Static(&'static str),
    Shared(Arc<str>),
}

const NO_MEMBER: u32 = u32::MAX;

/// Validates an absolute shape ID, returning the offsets of `#` and `$` (or [`NO_MEMBER`]).
///
/// This is the single implementation of the shape ID grammar. It is a `const fn` so that
/// [`ShapeId::from_static`] can reject invalid IDs at compile time.
const fn parse_absolute(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() >= NO_MEMBER as usize {
        return None;
    }
    let mut hash = None;
    let mut dollar = None;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'#' if hash.is_none() => hash = Some(i),
            b'$' if hash.is_some() && dollar.is_none() => dollar = Some(i),
            _ => {}
        }
        i += 1;
    }
    let Some(hash) = hash else { return None };
    if !valid_namespace(bytes, 0, hash) {
        return None;
    }
    match dollar {
        Some(dollar) => {
            if valid_identifier(bytes, hash + 1, dollar)
                && valid_identifier(bytes, dollar + 1, bytes.len())
            {
                Some((hash as u32, dollar as u32))
            } else {
                None
            }
        }
        None if valid_identifier(bytes, hash + 1, bytes.len()) => Some((hash as u32, NO_MEMBER)),
        None => None,
    }
}

/// Smithy identifier grammar (<https://smithy.io/2.0/spec/model.html#shape-id-abnf>) over
/// `bytes[start..end]`:
///
/// ```text
/// Identifier      = IdentifierStart *IdentifierChars
/// IdentifierStart = (1*"_" (ALPHA / DIGIT)) / ALPHA
/// IdentifierChars = ALPHA / DIGIT / "_"
/// ```
///
/// A digit may follow one or more leading underscores (`_1`), but an identifier without
/// leading underscores must start with a letter.
const fn valid_identifier(bytes: &[u8], start: usize, end: usize) -> bool {
    let mut i = start;
    while i < end && bytes[i] == b'_' {
        i += 1;
    }
    if i >= end {
        return false;
    }
    let first = bytes[i];
    if !(first.is_ascii_alphabetic() || (first.is_ascii_digit() && i > start)) {
        return false;
    }
    i += 1;
    while i < end {
        let b = bytes[i];
        if !(b.is_ascii_alphanumeric() || b == b'_') {
            return false;
        }
        i += 1;
    }
    true
}

/// `Namespace = Identifier *("." Identifier)` over `bytes[start..end]`.
const fn valid_namespace(bytes: &[u8], start: usize, end: usize) -> bool {
    let mut segment = start;
    let mut i = start;
    while i < end {
        if bytes[i] == b'.' {
            if !valid_identifier(bytes, segment, i) {
                return false;
            }
            segment = i + 1;
        }
        i += 1;
    }
    valid_identifier(bytes, segment, end)
}

impl ShapeId {
    /// Parses an absolute shape ID.
    pub fn new(absolute: &str) -> Result<Self, InvalidShapeIdError> {
        let (hash_index, dollar_index) = parse_absolute(absolute.as_bytes())
            .ok_or_else(|| InvalidShapeIdError::new(absolute))?;
        Ok(Self {
            repr: Repr::Shared(Arc::from(absolute)),
            hash_index,
            dollar_index,
        })
    }

    /// Creates a shape ID from a string literal, without allocating.
    ///
    /// This is a `const fn`, so it can initialize constants such as
    /// [`TraitCodec::ID`](crate::traits::TraitCodec::ID). An invalid ID is a compile-time
    /// error when used in a constant:
    ///
    /// ```
    /// use aws_smithy_lang::ShapeId;
    ///
    /// const RETRIES: ShapeId = ShapeId::from_static("com.example#retries");
    /// assert_eq!(RETRIES.name(), "retries");
    /// ```
    ///
    /// ```compile_fail
    /// # use aws_smithy_lang::ShapeId;
    /// const BAD: ShapeId = ShapeId::from_static("not a shape ID");
    /// # let _ = BAD;
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if `absolute` is not a valid absolute shape ID.
    pub const fn from_static(absolute: &'static str) -> Self {
        match parse_absolute(absolute.as_bytes()) {
            Some((hash_index, dollar_index)) => Self {
                repr: Repr::Static(absolute),
                hash_index,
                dollar_index,
            },
            None => panic!("invalid absolute shape ID"),
        }
    }

    /// Builds a shape ID from its namespace, name, and optional member name.
    pub fn from_parts(
        namespace: &str,
        name: &str,
        member: Option<&str>,
    ) -> Result<Self, InvalidShapeIdError> {
        let len = namespace.len() + 1 + name.len() + member.map_or(0, |m| m.len() + 1);
        let mut absolute = String::with_capacity(len);
        absolute.push_str(namespace);
        absolute.push('#');
        absolute.push_str(name);
        if let Some(member) = member {
            absolute.push('$');
            absolute.push_str(member);
        }
        Self::new(&absolute)
    }

    /// Parses `id`, resolving a relative ID (one without `#`) against `namespace`.
    ///
    /// Smithy JSON AST requires absolute IDs; this is for callers that hold relative names.
    pub fn from_relative(namespace: &str, id: &str) -> Result<Self, InvalidShapeIdError> {
        if id.contains('#') {
            Self::new(id)
        } else {
            Self::from_parts(namespace, id, None)
        }
    }

    /// The namespace, for example `com.example`. Borrowed; does not allocate.
    pub fn namespace(&self) -> &str {
        &self.as_str()[..self.hash_index as usize]
    }

    /// The shape name, without namespace or member, for example `Shape`. Borrowed; does not
    /// allocate.
    pub fn name(&self) -> &str {
        let end = if self.dollar_index == NO_MEMBER {
            self.as_str().len()
        } else {
            self.dollar_index as usize
        };
        &self.as_str()[self.hash_index as usize + 1..end]
    }

    /// The member name, if this is a member ID. Borrowed; does not allocate.
    pub fn member(&self) -> Option<&str> {
        (self.dollar_index != NO_MEMBER).then(|| &self.as_str()[self.dollar_index as usize + 1..])
    }

    /// Returns `true` if this ID has a member component.
    pub fn is_member(&self) -> bool {
        self.dollar_index != NO_MEMBER
    }

    /// The complete absolute ID. Borrowed; does not allocate.
    pub fn as_str(&self) -> &str {
        match &self.repr {
            Repr::Static(s) => s,
            Repr::Shared(s) => s,
        }
    }

    /// The absolute root ID as a borrowed slice, for map lookups without allocating.
    pub(crate) fn root_str(&self) -> &str {
        match self.dollar_index {
            NO_MEMBER => self.as_str(),
            end => &self.as_str()[..end as usize],
        }
    }

    /// The root shape ID: this ID without its member component.
    pub fn root(&self) -> ShapeId {
        if self.dollar_index == NO_MEMBER {
            return self.clone();
        }
        let end = self.dollar_index as usize;
        let repr = match &self.repr {
            Repr::Static(s) => Repr::Static(&s[..end]),
            Repr::Shared(s) => Repr::Shared(Arc::from(&s[..end])),
        };
        Self {
            repr,
            hash_index: self.hash_index,
            dollar_index: NO_MEMBER,
        }
    }

    /// Returns the ID of `member` within this ID's root shape.
    pub fn with_member(&self, member: &str) -> Result<ShapeId, InvalidShapeIdError> {
        let root = if self.dollar_index == NO_MEMBER {
            self.as_str()
        } else {
            &self.as_str()[..self.dollar_index as usize]
        };
        if !Self::is_valid_identifier(member) {
            return Err(InvalidShapeIdError::new(&format!("{root}${member}")));
        }
        let mut absolute = String::with_capacity(root.len() + 1 + member.len());
        absolute.push_str(root);
        absolute.push('$');
        absolute.push_str(member);
        Ok(Self {
            dollar_index: root.len() as u32,
            repr: Repr::Shared(Arc::from(absolute)),
            hash_index: self.hash_index,
        })
    }

    /// Returns `true` if `value` is a valid Smithy identifier.
    pub fn is_valid_identifier(value: &str) -> bool {
        valid_identifier(value.as_bytes(), 0, value.len())
    }

    /// Returns `true` if `value` is a valid Smithy namespace.
    pub fn is_valid_namespace(value: &str) -> bool {
        valid_namespace(value.as_bytes(), 0, value.len())
    }

    /// Returns `true` if both IDs share one allocation (or both are static).
    #[cfg(test)]
    pub(crate) fn shares_allocation(&self, other: &ShapeId) -> bool {
        match (&self.repr, &other.repr) {
            (Repr::Shared(a), Repr::Shared(b)) => Arc::ptr_eq(a, b),
            (Repr::Static(a), Repr::Static(b)) => std::ptr::eq(*a, *b),
            _ => false,
        }
    }
}

/// Values that can be converted into a [`ShapeId`]: a `ShapeId`, a `&ShapeId`, or a string to
/// parse.
///
/// Builders accept `impl IntoShapeId` and report an invalid ID when `build` is called, so
/// shapes can be written without parsing every ID up front.
pub trait IntoShapeId {
    /// Converts `self` into a shape ID.
    fn into_shape_id(self) -> Result<ShapeId, InvalidShapeIdError>;
}

impl IntoShapeId for ShapeId {
    fn into_shape_id(self) -> Result<ShapeId, InvalidShapeIdError> {
        Ok(self)
    }
}

impl IntoShapeId for &ShapeId {
    fn into_shape_id(self) -> Result<ShapeId, InvalidShapeIdError> {
        Ok(self.clone())
    }
}

impl IntoShapeId for &str {
    fn into_shape_id(self) -> Result<ShapeId, InvalidShapeIdError> {
        ShapeId::new(self)
    }
}

impl IntoShapeId for String {
    fn into_shape_id(self) -> Result<ShapeId, InvalidShapeIdError> {
        ShapeId::new(&self)
    }
}

impl IntoShapeId for &String {
    fn into_shape_id(self) -> Result<ShapeId, InvalidShapeIdError> {
        ShapeId::new(self)
    }
}

impl PartialEq for ShapeId {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
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
        self.as_str().cmp(other.as_str())
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
        self.as_str()
    }
}

impl AsRef<str> for ShapeId {
    fn as_ref(&self) -> &str {
        self.as_str()
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
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for ShapeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ShapeId({:?})", self.as_str())
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
        for ok in [
            "a#B",
            "__a.b_1#_C2",
            "a.b.c#D$e",
            "A#a$_x1",
            "a#B$_1x",
            "a#_1",
            "_1.a#B",
            "a#B$_1",
            "a#_0foo",
            "a#__2x",
            "_9._8#__7$_6",
        ] {
            assert!(ShapeId::new(ok).is_ok(), "{ok}");
        }
        for bad in [
            "", "#A", "a#", "a#B$", "a#$b", "a.#B", ".a#B", "a..b#C", "1a#B", "a#1B", "a#_",
            "a#__", "a#B$c$d", "a#B#C", "a#B c", "a#B-c", "a#Bé", "a", "a#B$1", "a#B$_", "a#B$_-",
            "1.a#B",
        ] {
            assert!(ShapeId::new(bad).is_err(), "{bad}");
        }
        assert!(ShapeId::is_valid_identifier("_1"));
        assert!(!ShapeId::is_valid_identifier("1"));
        assert!(ShapeId::is_valid_namespace("_1.a"));
        assert!(ShapeId::from_parts("a", "B", Some("c")).is_ok());
        assert!(ShapeId::from_parts("a", "B#", None).is_err());
        assert_eq!(ShapeId::from_relative("a.b", "C").unwrap(), "a.b#C");
        assert_eq!(ShapeId::from_relative("a.b", "x#C").unwrap(), "x#C");
    }

    mod properties {
        use super::*;
        use proptest::prelude::*;
        use std::collections::hash_map::DefaultHasher;

        /// Independent transcription of the Smithy ABNF used as an oracle.
        fn abnf_identifier(value: &str) -> bool {
            let chars: Vec<char> = value.chars().collect();
            let mut i = 0;
            while i < chars.len() && chars[i] == '_' {
                i += 1;
            }
            let start = match chars.get(i) {
                Some(c) if c.is_ascii_alphabetic() => true,
                Some(c) if c.is_ascii_digit() => i > 0,
                _ => false,
            };
            start
                && chars[i + 1..]
                    .iter()
                    .all(|c| c.is_ascii_alphanumeric() || *c == '_')
        }

        fn identifier() -> impl Strategy<Value = String> {
            prop_oneof![
                "[A-Za-z][A-Za-z0-9_]{0,8}",
                "_{1,3}[A-Za-z0-9][A-Za-z0-9_]{0,8}",
            ]
        }

        fn namespace() -> impl Strategy<Value = String> {
            prop::collection::vec(identifier(), 1..4).prop_map(|parts| parts.join("."))
        }

        fn hash_of<T: Hash + ?Sized>(value: &T) -> u64 {
            let mut hasher = DefaultHasher::new();
            value.hash(&mut hasher);
            hasher.finish()
        }

        proptest! {
            #[test]
            fn identifier_matches_abnf(value in "[A-Za-z0-9_.#$ -]{0,10}") {
                prop_assert_eq!(ShapeId::is_valid_identifier(&value), abnf_identifier(&value));
            }

            #[test]
            fn generated_ids_round_trip(
                ns in namespace(),
                name in identifier(),
                member in prop::option::of(identifier()),
            ) {
                let text = match &member {
                    Some(m) => format!("{ns}#{name}${m}"),
                    None => format!("{ns}#{name}"),
                };
                let id = ShapeId::new(&text).unwrap();
                prop_assert_eq!(id.to_string(), text.clone());
                prop_assert_eq!(id.namespace(), ns.as_str());
                prop_assert_eq!(id.name(), name.as_str());
                prop_assert_eq!(id.member(), member.as_deref());
                prop_assert_eq!(&ShapeId::from_parts(&ns, &name, member.as_deref()).unwrap(), &id);
                let reparsed: ShapeId = id.to_string().parse().unwrap();
                prop_assert_eq!(&reparsed, &id);
                // `Borrow<str>` requires hashing identically to the borrowed string.
                prop_assert_eq!(hash_of(&id), hash_of(text.as_str()));
                let mut map = HashMap::new();
                map.insert(id.clone(), ());
                prop_assert!(map.contains_key(text.as_str()));
            }
        }
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
    fn static_ids() {
        const ID: ShapeId = ShapeId::from_static("a.b#C$d");
        let parsed = ShapeId::new("a.b#C$d").unwrap();
        assert_eq!(ID, parsed);
        assert_eq!(
            (ID.namespace(), ID.name(), ID.member()),
            ("a.b", "C", Some("d"))
        );
        assert!(ID.root().shares_allocation(&ID.root()));
        assert_eq!(ID.root(), "a.b#C");
        let mut map = HashMap::new();
        map.insert(ID, 1);
        assert_eq!(map.get("a.b#C$d"), Some(&1));
        assert_eq!(map.get(&parsed), Some(&1));
        assert!(std::panic::catch_unwind(|| ShapeId::from_static("a#")).is_err());
    }

    #[test]
    fn into_shape_id() {
        fn id(value: impl IntoShapeId) -> Result<ShapeId, InvalidShapeIdError> {
            value.into_shape_id()
        }
        let expected = ShapeId::new("a#B").unwrap();
        assert_eq!(id("a#B").unwrap(), expected);
        assert_eq!(id(String::from("a#B")).unwrap(), expected);
        assert_eq!(id(&expected).unwrap(), expected);
        assert_eq!(id(expected.clone()).unwrap(), expected);
        assert_eq!(id("B").unwrap_err().value(), "B");
    }

    #[test]
    fn send_sync() {
        fn assert<T: Send + Sync + 'static>() {}
        assert::<ShapeId>();
        assert::<InvalidShapeIdError>();
    }
}
