/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Smithy node values: the JSON-like data model used by metadata and trait values.

use indexmap::IndexMap;
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};
use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};

/// A Smithy node value.
///
/// Equality and hashing of objects are independent of key order; arrays are ordered.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Node {
    /// `null`
    Null,
    /// `true` or `false`
    Bool(bool),
    /// A number.
    Number(Number),
    /// A string.
    String(String),
    /// An ordered array.
    Array(Vec<Node>),
    /// A string-keyed object.
    Object(NodeObject),
}

impl Node {
    /// Returns `true` for [`Node::Null`].
    pub fn is_null(&self) -> bool {
        matches!(self, Node::Null)
    }

    /// The boolean value, if this is a boolean.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Node::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// The number, if this is a number.
    pub fn as_number(&self) -> Option<&Number> {
        match self {
            Node::Number(n) => Some(n),
            _ => None,
        }
    }

    /// The string, if this is a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Node::String(s) => Some(s),
            _ => None,
        }
    }

    /// The elements, if this is an array.
    pub fn as_array(&self) -> Option<&[Node]> {
        match self {
            Node::Array(a) => Some(a),
            _ => None,
        }
    }

    /// The object, if this is an object.
    pub fn as_object(&self) -> Option<&NodeObject> {
        match self {
            Node::Object(o) => Some(o),
            _ => None,
        }
    }

    /// The boolean value, or an error naming the actual node type.
    pub fn expect_bool(&self) -> Result<bool, NodeTypeError> {
        self.as_bool().ok_or_else(|| self.type_error("boolean"))
    }

    /// The number, or an error naming the actual node type.
    pub fn expect_number(&self) -> Result<&Number, NodeTypeError> {
        self.as_number().ok_or_else(|| self.type_error("number"))
    }

    /// The string, or an error naming the actual node type.
    pub fn expect_str(&self) -> Result<&str, NodeTypeError> {
        self.as_str().ok_or_else(|| self.type_error("string"))
    }

    /// The elements, or an error naming the actual node type.
    pub fn expect_array(&self) -> Result<&[Node], NodeTypeError> {
        self.as_array().ok_or_else(|| self.type_error("array"))
    }

    /// The object, or an error naming the actual node type.
    pub fn expect_object(&self) -> Result<&NodeObject, NodeTypeError> {
        self.as_object().ok_or_else(|| self.type_error("object"))
    }

    /// The name of this node's type: `null`, `boolean`, `number`, `string`, `array`, or `object`.
    pub fn type_name(&self) -> &'static str {
        match self {
            Node::Null => "null",
            Node::Bool(_) => "boolean",
            Node::Number(_) => "number",
            Node::String(_) => "string",
            Node::Array(_) => "array",
            Node::Object(_) => "object",
        }
    }

    fn type_error(&self, expected: &'static str) -> NodeTypeError {
        NodeTypeError {
            expected,
            actual: self.type_name(),
        }
    }

    /// Looks up a value by [RFC 6901](https://www.rfc-editor.org/rfc/rfc6901) JSON Pointer.
    ///
    /// The empty pointer returns `self`. Returns `None` for a malformed pointer or missing value.
    pub fn pointer(&self, pointer: &str) -> Option<&Node> {
        if pointer.is_empty() {
            return Some(self);
        }
        let rest = pointer.strip_prefix('/')?;
        let mut current = self;
        for token in rest.split('/') {
            let token = unescape_pointer_token(token)?;
            current = match current {
                Node::Object(object) => object.get(&token)?,
                Node::Array(array) => {
                    if token.len() > 1 && token.starts_with('0') {
                        return None;
                    }
                    array.get(token.parse::<usize>().ok()?)?
                }
                _ => return None,
            };
        }
        Some(current)
    }

    /// Returns a copy with every nested object's keys sorted.
    pub fn with_sorted_keys(&self) -> Node {
        let mut node = self.clone();
        node.sort_keys();
        node
    }

    /// Sorts every nested object's keys in place.
    pub fn sort_keys(&mut self) {
        match self {
            Node::Array(array) => array.iter_mut().for_each(Node::sort_keys),
            Node::Object(object) => {
                object.0.sort_unstable_keys();
                object.0.values_mut().for_each(Node::sort_keys);
            }
            _ => {}
        }
    }
}

fn unescape_pointer_token(token: &str) -> Option<String> {
    let mut out = String::with_capacity(token.len());
    let mut chars = token.chars();
    while let Some(c) = chars.next() {
        if c == '~' {
            match chars.next()? {
                '0' => out.push('~'),
                '1' => out.push('/'),
                _ => return None,
            }
        } else {
            out.push(c);
        }
    }
    Some(out)
}

impl From<bool> for Node {
    fn from(value: bool) -> Self {
        Node::Bool(value)
    }
}

impl From<&str> for Node {
    fn from(value: &str) -> Self {
        Node::String(value.to_owned())
    }
}

impl From<String> for Node {
    fn from(value: String) -> Self {
        Node::String(value)
    }
}

impl From<Number> for Node {
    fn from(value: Number) -> Self {
        Node::Number(value)
    }
}

impl From<i64> for Node {
    fn from(value: i64) -> Self {
        Node::Number(Number::from(value))
    }
}

impl From<u64> for Node {
    fn from(value: u64) -> Self {
        Node::Number(Number::from(value))
    }
}

impl From<Vec<Node>> for Node {
    fn from(value: Vec<Node>) -> Self {
        Node::Array(value)
    }
}

impl From<NodeObject> for Node {
    fn from(value: NodeObject) -> Self {
        Node::Object(value)
    }
}

/// A node had a different type than required.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeTypeError {
    expected: &'static str,
    actual: &'static str,
}

impl NodeTypeError {
    /// The required node type.
    pub fn expected(&self) -> &'static str {
        self.expected
    }

    /// The actual node type.
    pub fn actual(&self) -> &'static str {
        self.actual
    }
}

impl fmt::Display for NodeTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "expected {} node but found {}",
            self.expected, self.actual
        )
    }
}

impl std::error::Error for NodeTypeError {}

/// An insertion-ordered map from strings to nodes.
///
/// Equality and hashing ignore insertion order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeObject(IndexMap<String, Node>);

impl NodeObject {
    /// Creates an empty object.
    pub fn new() -> Self {
        Self::default()
    }

    /// The number of entries.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` if the object has no entries.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Looks up a value by key.
    pub fn get(&self, key: &str) -> Option<&Node> {
        self.0.get(key)
    }

    /// Returns `true` if `key` is present.
    pub fn contains_key(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    /// Inserts or replaces a value, returning the previous value. New keys are appended.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<Node>) -> Option<Node> {
        self.0.insert(key.into(), value.into())
    }

    /// Removes a value, preserving the order of the remaining entries.
    pub fn remove(&mut self, key: &str) -> Option<Node> {
        self.0.shift_remove(key)
    }

    /// Iterates entries in insertion order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, &Node)> + '_ {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Iterates keys in insertion order.
    pub fn keys(&self) -> impl ExactSizeIterator<Item = &str> + '_ {
        self.0.keys().map(String::as_str)
    }
}

impl NodeObject {
    pub(crate) fn into_entries(self) -> impl Iterator<Item = (String, Node)> {
        self.0.into_iter()
    }
}

impl Hash for NodeObject {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Order-independent: combine per-entry hashes commutatively.
        let mut combined = 0u64;
        for entry in &self.0 {
            let mut hasher = DefaultHasher::new();
            entry.hash(&mut hasher);
            combined = combined.wrapping_add(hasher.finish());
        }
        self.0.len().hash(state);
        combined.hash(state);
    }
}

impl<K: Into<String>, V: Into<Node>> FromIterator<(K, V)> for NodeObject {
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self {
        Self(
            iter.into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        )
    }
}

/// A JSON number.
///
/// Integers parsed from JSON stay exact when they fit in `i64` or `u64`; every other number
/// is a finite `f64`. Integer and floating-point numbers are never equal to each other, so
/// `1` and `1.0` are distinct. Negative zero is normalized to positive zero.
#[derive(Clone, Copy)]
pub struct Number(NumberRepr);

#[derive(Clone, Copy)]
enum NumberRepr {
    /// Non-negative integer.
    PosInt(u64),
    /// Negative integer.
    NegInt(i64),
    /// Finite, never negative zero.
    Float(f64),
}

impl Number {
    /// Creates a floating-point number. Returns `None` for NaN or infinities.
    pub fn from_f64(value: f64) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }
        // Normalize -0.0 so equal values can never hash differently.
        let value = if value == 0.0 { 0.0 } else { value };
        Some(Self(NumberRepr::Float(value)))
    }

    /// Returns `true` if this number is an integer (rather than floating-point).
    pub fn is_integer(&self) -> bool {
        !matches!(self.0, NumberRepr::Float(_))
    }

    /// Returns `true` if this number is floating-point.
    pub fn is_float(&self) -> bool {
        matches!(self.0, NumberRepr::Float(_))
    }

    /// The value as `i64` if it is an integer that fits.
    pub fn as_i64(&self) -> Option<i64> {
        match self.0 {
            NumberRepr::PosInt(v) => i64::try_from(v).ok(),
            NumberRepr::NegInt(v) => Some(v),
            NumberRepr::Float(_) => None,
        }
    }

    /// The value as `u64` if it is a non-negative integer.
    pub fn as_u64(&self) -> Option<u64> {
        match self.0 {
            NumberRepr::PosInt(v) => Some(v),
            _ => None,
        }
    }

    /// The value as `f64`. Large integers may lose precision.
    pub fn as_f64(&self) -> f64 {
        match self.0 {
            NumberRepr::PosInt(v) => v as f64,
            NumberRepr::NegInt(v) => v as f64,
            NumberRepr::Float(v) => v,
        }
    }
}

impl From<u64> for Number {
    fn from(value: u64) -> Self {
        Self(NumberRepr::PosInt(value))
    }
}

impl From<i64> for Number {
    fn from(value: i64) -> Self {
        if value >= 0 {
            Self(NumberRepr::PosInt(value as u64))
        } else {
            Self(NumberRepr::NegInt(value))
        }
    }
}

impl From<i32> for Number {
    fn from(value: i32) -> Self {
        Self::from(i64::from(value))
    }
}

impl PartialEq for Number {
    fn eq(&self, other: &Self) -> bool {
        match (self.0, other.0) {
            (NumberRepr::PosInt(a), NumberRepr::PosInt(b)) => a == b,
            (NumberRepr::NegInt(a), NumberRepr::NegInt(b)) => a == b,
            // Bitwise comparison is equivalent to `==` for finite floats without -0.0.
            (NumberRepr::Float(a), NumberRepr::Float(b)) => a.to_bits() == b.to_bits(),
            _ => false,
        }
    }
}

impl Eq for Number {}

impl Hash for Number {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self.0 {
            NumberRepr::PosInt(v) => (0u8, v).hash(state),
            NumberRepr::NegInt(v) => (1u8, v).hash(state),
            NumberRepr::Float(v) => (2u8, v.to_bits()).hash(state),
        }
    }
}

impl fmt::Debug for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Number({self})")
    }
}

impl fmt::Display for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            NumberRepr::PosInt(v) => write!(f, "{v}"),
            NumberRepr::NegInt(v) => write!(f, "{v}"),
            NumberRepr::Float(v) => {
                let json = serde_json::to_string(&v).map_err(|_| fmt::Error)?;
                f.write_str(&json)
            }
        }
    }
}

/// Serializes a [`Node`] in insertion order.
///
/// This is a crate-private wrapper rather than a public `Serialize` impl, so that `serde` stays
/// out of the public API. The writer sorts object keys itself and only uses this for scalars.
pub(crate) struct NodeSer<'a>(pub(crate) &'a Node);

impl Serialize for NodeSer<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Node::Null => serializer.serialize_unit(),
            Node::Bool(b) => serializer.serialize_bool(*b),
            Node::Number(n) => match n.0 {
                NumberRepr::PosInt(v) => serializer.serialize_u64(v),
                NumberRepr::NegInt(v) => serializer.serialize_i64(v),
                NumberRepr::Float(v) => serializer.serialize_f64(v),
            },
            Node::String(s) => serializer.serialize_str(s),
            Node::Array(array) => {
                let mut seq = serializer.serialize_seq(Some(array.len()))?;
                for element in array {
                    seq.serialize_element(&NodeSer(element))?;
                }
                seq.end()
            }
            Node::Object(object) => {
                let mut map = serializer.serialize_map(Some(object.len()))?;
                for (k, v) in &object.0 {
                    map.serialize_entry(k, &NodeSer(v))?;
                }
                map.end()
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------------------------

/// Why a parse aborted, beyond ordinary JSON syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ParseAbort {
    DuplicateKey(String),
    DepthLimit(usize),
    ShapeLimit(usize),
}

/// Shared state for one parse: limits and the first structural abort reason.
#[derive(Debug)]
pub(crate) struct ParseContext {
    pub(crate) max_depth: usize,
    pub(crate) abort: RefCell<Option<ParseAbort>>,
}

impl ParseContext {
    pub(crate) fn new(max_depth: usize) -> Self {
        Self {
            max_depth,
            abort: RefCell::new(None),
        }
    }

    pub(crate) fn fail<E: de::Error>(&self, abort: ParseAbort) -> E {
        let message = match &abort {
            ParseAbort::DuplicateKey(key) => format!("duplicate object key `{key}`"),
            ParseAbort::DepthLimit(max) => format!("nesting exceeds the maximum depth of {max}"),
            ParseAbort::ShapeLimit(max) => format!("document exceeds the maximum of {max} shapes"),
        };
        self.abort.borrow_mut().get_or_insert(abort);
        E::custom(message)
    }
}

/// Deserializes a [`Node`] at `depth`, rejecting duplicate keys and excessive nesting.
pub(crate) struct NodeSeed<'c> {
    pub(crate) ctx: &'c ParseContext,
    pub(crate) depth: usize,
}

impl<'de> DeserializeSeed<'de> for NodeSeed<'_> {
    type Value = Node;

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<Node, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl NodeSeed<'_> {
    fn child<E: de::Error>(&self) -> Result<Self, E> {
        if self.depth >= self.ctx.max_depth {
            return Err(self.ctx.fail(ParseAbort::DepthLimit(self.ctx.max_depth)));
        }
        Ok(NodeSeed {
            ctx: self.ctx,
            depth: self.depth + 1,
        })
    }
}

impl<'de> Visitor<'de> for NodeSeed<'_> {
    type Value = Node;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_unit<E>(self) -> Result<Node, E> {
        Ok(Node::Null)
    }

    fn visit_bool<E>(self, v: bool) -> Result<Node, E> {
        Ok(Node::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Node, E> {
        Ok(Node::Number(Number::from(v)))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Node, E> {
        Ok(Node::Number(Number::from(v)))
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Node, E> {
        Number::from_f64(v)
            .map(Node::Number)
            .ok_or_else(|| E::custom("non-finite number"))
    }

    fn visit_str<E>(self, v: &str) -> Result<Node, E> {
        Ok(Node::String(v.to_owned()))
    }

    fn visit_string<E>(self, v: String) -> Result<Node, E> {
        Ok(Node::String(v))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Node, A::Error> {
        let child = self.child()?;
        let mut out = Vec::new();
        while let Some(value) = seq.next_element_seed(NodeSeed { ..child })? {
            out.push(value);
        }
        Ok(Node::Array(out))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
        let child = self.child()?;
        let mut out = IndexMap::new();
        while let Some(key) = map.next_key::<String>()? {
            if out.contains_key(&key) {
                return Err(self.ctx.fail(ParseAbort::DuplicateKey(key)));
            }
            let value = map.next_value_seed(NodeSeed { ..child })?;
            out.insert(key, value);
        }
        Ok(Node::Object(NodeObject(out)))
    }
}

#[cfg(test)]
pub(crate) fn parse_node(json: &str) -> Result<Node, serde_json::Error> {
    let ctx = ParseContext::new(64);
    let mut de = serde_json::Deserializer::from_str(json);
    let node = NodeSeed {
        ctx: &ctx,
        depth: 0,
    }
    .deserialize(&mut de)?;
    de.end()?;
    Ok(node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn hash_of<T: Hash>(t: &T) -> u64 {
        let mut h = DefaultHasher::new();
        t.hash(&mut h);
        h.finish()
    }

    #[test]
    fn numbers() {
        let one = parse_node("1").unwrap();
        let one_f = parse_node("1.0").unwrap();
        assert_ne!(one, one_f);
        assert_eq!(Number::from(1i64), Number::from(1u64));
        assert_eq!(hash_of(&Number::from(1i64)), hash_of(&Number::from(1u64)));
        let neg = Number::from_f64(-0.0).unwrap();
        let pos = Number::from_f64(0.0).unwrap();
        assert_eq!(neg, pos);
        assert_eq!(hash_of(&neg), hash_of(&pos));
        assert_eq!(neg.to_string(), "0.0");
        assert!(Number::from_f64(f64::NAN).is_none());
        assert!(Number::from_f64(f64::INFINITY).is_none());
        let max = parse_node(&u64::MAX.to_string()).unwrap();
        assert_eq!(max.as_number().unwrap().as_u64(), Some(u64::MAX));
        assert_eq!(max.as_number().unwrap().as_i64(), None);
        let min = parse_node(&i64::MIN.to_string()).unwrap();
        assert_eq!(min.as_number().unwrap().as_i64(), Some(i64::MIN));
        assert!(parse_node("1e999").is_err());
        assert_eq!(parse_node("1e2").unwrap().to_string_json(), "100.0");
        assert_eq!(parse_node("-5").unwrap().to_string_json(), "-5");
    }

    impl Node {
        fn to_string_json(&self) -> String {
            serde_json::to_string(&NodeSer(self)).unwrap()
        }
    }

    #[test]
    fn duplicate_keys_rejected_at_every_depth() {
        for json in [r#"{"a":1,"a":2}"#, r#"{"x":[{"b":{"c":1,"c":1}}]}"#] {
            let err = parse_node(json).unwrap_err();
            assert!(err.to_string().contains("duplicate object key"), "{err}");
        }
    }

    #[test]
    fn depth_limit() {
        let ctx = ParseContext::new(3);
        let mut de = serde_json::Deserializer::from_str("[[[[1]]]]");
        assert!(NodeSeed {
            ctx: &ctx,
            depth: 0
        }
        .deserialize(&mut de)
        .is_err());
        assert_eq!(*ctx.abort.borrow(), Some(ParseAbort::DepthLimit(3)));
        let ctx = ParseContext::new(3);
        let mut de = serde_json::Deserializer::from_str("[[[1]]]");
        assert!(NodeSeed {
            ctx: &ctx,
            depth: 0
        }
        .deserialize(&mut de)
        .is_ok());
    }

    #[test]
    fn object_equality_ignores_order() {
        let a = parse_node(r#"{"a":1,"b":[1,2]}"#).unwrap();
        let b = parse_node(r#"{"b":[1,2],"a":1}"#).unwrap();
        let c = parse_node(r#"{"b":[2,1],"a":1}"#).unwrap();
        assert_eq!(a, b);
        assert_eq!(hash_of(&a), hash_of(&b));
        assert_ne!(a, c);
        assert_eq!(
            b.with_sorted_keys().to_string_json(),
            r#"{"a":1,"b":[1,2]}"#
        );
    }

    #[test]
    fn pointers_and_accessors() {
        let n = parse_node(r#"{"a/b":{"~x":[10,true,null]}}"#).unwrap();
        assert_eq!(n.pointer(""), Some(&n));
        assert_eq!(n.pointer("/a~1b/~0x/1"), Some(&Node::Bool(true)));
        assert!(n.pointer("/a~1b/~0x/01").is_none());
        assert!(n.pointer("/a~1b/~2").is_none());
        assert!(n.pointer("a").is_none());
        assert!(n.pointer("/a~1b/~0x/2").unwrap().is_null());
        let err = n.expect_array().unwrap_err();
        assert_eq!((err.expected(), err.actual()), ("array", "object"));
        assert_eq!(err.to_string(), "expected array node but found object");
    }

    fn arb_node() -> impl Strategy<Value = Node> {
        let leaf = prop_oneof![
            Just(Node::Null),
            any::<bool>().prop_map(Node::Bool),
            any::<i64>().prop_map(Node::from),
            any::<u64>().prop_map(Node::from),
            (-1e300f64..1e300).prop_map(|f| Node::Number(Number::from_f64(f).unwrap())),
            ".{0,8}".prop_map(Node::String),
        ];
        leaf.prop_recursive(4, 32, 6, |inner| {
            prop_oneof![
                prop::collection::vec(inner.clone(), 0..6).prop_map(Node::Array),
                prop::collection::vec(("[a-c]{0,2}", inner), 0..6)
                    .prop_map(|entries| Node::Object(entries.into_iter().collect())),
            ]
        })
    }

    proptest! {
        #[test]
        fn json_round_trip(node in arb_node()) {
            let json = serde_json::to_string(&NodeSer(&node)).unwrap();
            let back = parse_node(&json).unwrap();
            prop_assert_eq!(&back, &node);
            prop_assert_eq!(hash_of(&back), hash_of(&node));
        }

        #[test]
        fn eq_implies_equal_hash(a in arb_node(), b in arb_node()) {
            if a == b {
                prop_assert_eq!(hash_of(&a), hash_of(&b));
            }
            let sorted = a.with_sorted_keys();
            prop_assert_eq!(&sorted, &a);
            prop_assert_eq!(hash_of(&sorted), hash_of(&a));
        }
    }
}
