/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Smithy node values: the JSON-like data model used by metadata and trait values.

use crate::diagnostic::{fmt_located, push_pointer_token, LocationTable, Position, SourceLocation};
use indexmap::IndexMap;
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};
use std::cell::{Cell, RefCell};
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// A Smithy node value: the JSON-like data model used by metadata and trait values.
///
/// A node pairs a [`NodeValue`] with the [`SourceLocation`] it was loaded from. Nodes loaded
/// from a model record the JSON Pointer of every nested value, so trait codecs and validators
/// can report exactly which value is wrong. Nodes created in code have
/// [`SourceLocation::none`] unless one is attached with
/// [`with_source_location`](Self::with_source_location).
///
/// Equality and hashing ignore source locations. Object equality and hashing are independent
/// of key order; arrays are ordered.
///
/// ```
/// use aws_smithy_lang::node::{Node, NodeValue};
///
/// let node = Node::from(vec![Node::from("a"), Node::from(1i64)]);
/// match node.value() {
///     NodeValue::Array(items) => assert_eq!(items[0].as_str(), Some("a")),
///     _ => unreachable!(),
/// }
/// assert!(node.source_location().is_none());
/// ```
#[derive(Clone)]
pub struct Node {
    value: NodeValue,
    location: SourceLocation,
}

/// The value of a [`Node`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NodeValue {
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
    /// Creates a node with no source location.
    pub fn new(value: NodeValue) -> Self {
        Self {
            value,
            location: SourceLocation::none(),
        }
    }

    /// A `null` node.
    pub fn null() -> Self {
        Self::new(NodeValue::Null)
    }

    /// Returns this node with `location` as its source location. Nested values keep their own
    /// locations.
    pub fn with_source_location(mut self, location: SourceLocation) -> Self {
        self.location = location;
        self
    }

    /// The value.
    pub fn value(&self) -> &NodeValue {
        &self.value
    }

    /// Consumes the node, returning its value.
    pub fn into_value(self) -> NodeValue {
        self.value
    }

    /// Where this value was loaded from, or [`SourceLocation::none`].
    pub fn source_location(&self) -> &SourceLocation {
        &self.location
    }

    /// Returns `true` for a `null` node.
    pub fn is_null(&self) -> bool {
        matches!(self.value, NodeValue::Null)
    }

    /// The boolean value, if this is a boolean.
    pub fn as_bool(&self) -> Option<bool> {
        match self.value {
            NodeValue::Bool(b) => Some(b),
            _ => None,
        }
    }

    /// The number, if this is a number.
    pub fn as_number(&self) -> Option<&Number> {
        match &self.value {
            NodeValue::Number(n) => Some(n),
            _ => None,
        }
    }

    /// The string, if this is a string.
    pub fn as_str(&self) -> Option<&str> {
        match &self.value {
            NodeValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// The elements, if this is an array.
    pub fn as_array(&self) -> Option<&[Node]> {
        match &self.value {
            NodeValue::Array(a) => Some(a),
            _ => None,
        }
    }

    /// The object, if this is an object.
    pub fn as_object(&self) -> Option<&NodeObject> {
        match &self.value {
            NodeValue::Object(o) => Some(o),
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
        match self.value {
            NodeValue::Null => "null",
            NodeValue::Bool(_) => "boolean",
            NodeValue::Number(_) => "number",
            NodeValue::String(_) => "string",
            NodeValue::Array(_) => "array",
            NodeValue::Object(_) => "object",
        }
    }

    fn type_error(&self, expected: &'static str) -> NodeTypeError {
        NodeTypeError {
            expected,
            actual: self.type_name(),
            location: self.location.clone(),
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
            current = match &current.value {
                NodeValue::Object(object) => object.get(&token)?,
                NodeValue::Array(array) => {
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
        match &mut self.value {
            NodeValue::Array(array) => array.iter_mut().for_each(Node::sort_keys),
            NodeValue::Object(object) => {
                object.0.sort_unstable_keys();
                object.0.values_mut().for_each(Node::sort_keys);
            }
            _ => {}
        }
    }

    /// Serializes this node as compact JSON, keeping object keys in insertion order.
    ///
    /// Numbers use their canonical spelling (`1e2` is written as `100.0`). Call
    /// [`with_sorted_keys`](Self::with_sorted_keys) first for output independent of key order.
    pub fn to_json_string(&self) -> String {
        // Infallible: keys are strings and numbers are finite.
        serde_json::to_string(&NodeSer(self)).expect("nodes always serialize")
    }

    /// Serializes this node as indented JSON. See [`to_json_string`](Self::to_json_string).
    pub fn to_json_string_pretty(&self) -> String {
        serde_json::to_string_pretty(&NodeSer(self)).expect("nodes always serialize")
    }

    /// Records the JSON Pointer of this node and every nested value, relative to `pointer`.
    ///
    /// All the pointers share one table, so this allocates a constant number of times
    /// regardless of how many values the tree contains.
    /// Gives this node and every nested value a location in `source` at `pointer`.
    ///
    /// Each value keeps the line and column recorded while parsing; values without one use
    /// the closest enclosing value's position, falling back to `inherited`.
    pub(crate) fn assign_locations(
        &mut self,
        source: &Arc<str>,
        pointer: &mut String,
        inherited: Option<Position>,
    ) {
        let position = self.location.position().or(inherited);
        if !matches!(self.value, NodeValue::Array(_) | NodeValue::Object(_)) {
            // A scalar needs one location, not a table.
            self.location = SourceLocation::with_pointer(source, pointer.clone(), position);
            return;
        }
        let mut table = LocationTable::new(source.clone());
        self.collect_pointers(&mut table, pointer, position);
        let table = Arc::new(table);
        let mut next = 0;
        self.set_locations(&table, &mut next);
    }

    /// Records pointers in preorder: this node, then its elements or entries.
    fn collect_pointers(
        &self,
        table: &mut LocationTable,
        pointer: &mut String,
        inherited: Option<Position>,
    ) {
        let position = self.location.position().or(inherited);
        table.push(pointer, position);
        let len = pointer.len();
        match &self.value {
            NodeValue::Array(array) => {
                let mut digits = itoa_buffer();
                for (i, element) in array.iter().enumerate() {
                    pointer.push('/');
                    pointer.push_str(format_index(&mut digits, i));
                    element.collect_pointers(table, pointer, position);
                    pointer.truncate(len);
                }
            }
            NodeValue::Object(object) => {
                for (key, value) in object.0.iter() {
                    pointer.push('/');
                    push_pointer_token(pointer, key);
                    value.collect_pointers(table, pointer, position);
                    pointer.truncate(len);
                }
            }
            _ => {}
        }
    }

    /// The line and column recorded while parsing, or of this node's location.
    pub(crate) fn position(&self) -> Option<Position> {
        self.location.position()
    }

    /// Records a parse-time position, completed later by [`assign_locations`](Self::assign_locations).
    pub(crate) fn with_position(mut self, position: Option<Position>) -> Self {
        if let Some(position) = position {
            self.location = SourceLocation::pending(position);
        }
        self
    }

    /// Assigns table entries in the same preorder as [`collect_pointers`](Self::collect_pointers).
    fn set_locations(&mut self, table: &Arc<LocationTable>, next: &mut u32) {
        self.location = LocationTable::location(table, *next);
        *next += 1;
        match &mut self.value {
            NodeValue::Array(array) => array.iter_mut().for_each(|e| e.set_locations(table, next)),
            NodeValue::Object(object) => object
                .0
                .values_mut()
                .for_each(|v| v.set_locations(table, next)),
            _ => {}
        }
    }
}

fn itoa_buffer() -> [u8; 20] {
    [0; 20]
}

/// Formats `value` in decimal into `buffer` without allocating.
fn format_index(buffer: &mut [u8; 20], mut value: usize) -> &str {
    let mut i = buffer.len();
    loop {
        i -= 1;
        buffer[i] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    std::str::from_utf8(&buffer[i..]).expect("ASCII digits")
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Eq for Node {}

impl Hash for Node {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.value.hash(state)
    }
}

impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Locations are omitted to keep nested output readable; see `source_location`.
        fmt::Debug::fmt(&self.value, f)
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

impl From<NodeValue> for Node {
    fn from(value: NodeValue) -> Self {
        Node::new(value)
    }
}

impl From<bool> for Node {
    fn from(value: bool) -> Self {
        Node::new(NodeValue::Bool(value))
    }
}

impl From<&str> for Node {
    fn from(value: &str) -> Self {
        Node::new(NodeValue::String(value.to_owned()))
    }
}

impl From<String> for Node {
    fn from(value: String) -> Self {
        Node::new(NodeValue::String(value))
    }
}

impl From<Number> for Node {
    fn from(value: Number) -> Self {
        Node::new(NodeValue::Number(value))
    }
}

impl From<i64> for Node {
    fn from(value: i64) -> Self {
        Node::from(Number::from(value))
    }
}

impl From<u64> for Node {
    fn from(value: u64) -> Self {
        Node::from(Number::from(value))
    }
}

impl From<Vec<Node>> for Node {
    fn from(value: Vec<Node>) -> Self {
        Node::new(NodeValue::Array(value))
    }
}

impl From<NodeObject> for Node {
    fn from(value: NodeObject) -> Self {
        Node::new(NodeValue::Object(value))
    }
}

/// A node had a different type than required.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeTypeError {
    expected: &'static str,
    actual: &'static str,
    location: SourceLocation,
}

impl NodeTypeError {
    /// Where the offending node was loaded from, or [`SourceLocation::none`].
    pub fn source_location(&self) -> &SourceLocation {
        &self.location
    }

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
        fmt_located(
            f,
            &self.location,
            format_args!("expected {} node but found {}", self.expected, self.actual),
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
        match &self.0.value {
            NodeValue::Null => serializer.serialize_unit(),
            NodeValue::Bool(b) => serializer.serialize_bool(*b),
            NodeValue::Number(n) => match n.0 {
                NumberRepr::PosInt(v) => serializer.serialize_u64(v),
                NumberRepr::NegInt(v) => serializer.serialize_i64(v),
                NumberRepr::Float(v) => serializer.serialize_f64(v),
            },
            NodeValue::String(s) => serializer.serialize_str(s),
            NodeValue::Array(array) => {
                let mut seq = serializer.serialize_seq(Some(array.len()))?;
                for element in array {
                    seq.serialize_element(&NodeSer(element))?;
                }
                seq.end()
            }
            NodeValue::Object(object) => {
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

/// Map key serde_json uses to tunnel numbers through `visit_map` when its additive
/// `arbitrary_precision` feature is enabled anywhere in the dependency graph.
const ARBITRARY_PRECISION_TOKEN: &str = "$serde_json::private::Number";

/// Returns `true` if serde_json was compiled with `arbitrary_precision`.
///
/// Cargo feature unification means a downstream crate can enable that feature without this
/// crate's knowledge. In that mode serde_json reports every non-integer number, and every integer
/// outside `i64`/`u64`, as a one-entry map keyed by [`ARBITRARY_PRECISION_TOKEN`]. Detecting the
/// mode lets the node parser decode those numbers while still treating an object that merely
/// uses that key as an ordinary object when the feature is off.
fn serde_json_arbitrary_precision() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        struct Probe;
        impl<'de> Visitor<'de> for Probe {
            type Value = bool;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a number")
            }
            fn visit_f64<E>(self, _: f64) -> Result<bool, E> {
                Ok(false)
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<bool, A::Error> {
                // Drain the entry so the deserializer stays consistent.
                while map.next_entry::<String, String>()?.is_some() {}
                Ok(true)
            }
        }
        let mut de = serde_json::Deserializer::from_str("0.5");
        de::Deserializer::deserialize_any(&mut de, Probe).unwrap_or(false)
    })
}

/// Converts the textual number serde_json reports in `arbitrary_precision` mode into a
/// [`Number`] with the same representation the default build would have produced: exact
/// `i64`/`u64` integers where they fit, otherwise a finite `f64`.
fn parse_number_text<E: de::Error>(text: &str) -> Result<Number, E> {
    let is_integer = !text.contains(['.', 'e', 'E']);
    if is_integer {
        if let Ok(value) = text.parse::<u64>() {
            return Ok(Number::from(value));
        }
        if let Ok(value) = text.parse::<i64>() {
            return Ok(Number::from(value));
        }
    }
    text.parse::<f64>()
        .ok()
        .and_then(Number::from_f64)
        .ok_or_else(|| E::custom("number out of range"))
}

/// Shared state for one parse: limits, the first structural abort reason, and the input used
/// to turn borrowed keys and strings into line and column positions.
#[derive(Debug)]
pub(crate) struct ParseContext<'i> {
    pub(crate) max_depth: usize,
    pub(crate) abort: RefCell<Option<ParseAbort>>,
    /// Whether maps keyed by [`ARBITRARY_PRECISION_TOKEN`] are serde_json numbers.
    arbitrary_precision: bool,
    input: &'i [u8],
    lines: Cell<LineCursor>,
}

/// The line containing the last position computed. Parsing moves forward, so counting
/// newlines from here makes computing every position O(input) in total.
#[derive(Debug, Clone, Copy)]
struct LineCursor {
    offset: usize,
    line: u32,
    line_start: usize,
}

impl LineCursor {
    const START: Self = Self {
        offset: 0,
        line: 1,
        line_start: 0,
    };
}

impl<'i> ParseContext<'i> {
    /// `input` is the document being parsed; strings borrowed from it get positions.
    pub(crate) fn new(max_depth: usize, input: &'i [u8]) -> Self {
        Self {
            max_depth,
            abort: RefCell::new(None),
            arbitrary_precision: serde_json_arbitrary_precision(),
            input,
            lines: Cell::new(LineCursor::START),
        }
    }

    /// The position of the opening quote of `text`, a string serde_json borrowed from the
    /// input. Returns `None` for strings that were unescaped into a separate buffer.
    pub(crate) fn position_of(&self, text: &str) -> Option<Position> {
        let base = self.input.as_ptr() as usize;
        let start = (text.as_ptr() as usize).checked_sub(base)?;
        if start == 0 || start + text.len() > self.input.len() {
            return None;
        }
        Some(self.position_at(start - 1))
    }

    fn position_at(&self, offset: usize) -> Position {
        let mut cursor = self.lines.get();
        if offset < cursor.offset {
            cursor = LineCursor::START;
        }
        let skipped = &self.input[cursor.offset..offset];
        let newlines = skipped.iter().filter(|&&b| b == b'\n').count();
        if newlines > 0 {
            cursor.line = cursor
                .line
                .saturating_add(u32::try_from(newlines).unwrap_or(u32::MAX));
            let last = skipped
                .iter()
                .rposition(|&b| b == b'\n')
                .expect("counted a newline");
            cursor.line_start = cursor.offset + last + 1;
        }
        cursor.offset = offset;
        self.lines.set(cursor);
        // Columns count characters: every byte that is not a UTF-8 continuation byte.
        let column = self.input[cursor.line_start..offset]
            .iter()
            .filter(|&&b| b & 0xC0 != 0x80)
            .count()
            + 1;
        Position {
            line: cursor.line,
            column: u32::try_from(column).unwrap_or(u32::MAX),
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
///
/// Values introduced by an object key get the key's position; strings in arrays get their own.
pub(crate) struct NodeSeed<'c> {
    pub(crate) ctx: &'c ParseContext<'c>,
    pub(crate) depth: usize,
}

/// Deserializes an object key with its position, when serde_json borrowed it from the input.
pub(crate) struct KeySeed<'c> {
    pub(crate) ctx: &'c ParseContext<'c>,
}

impl<'de> DeserializeSeed<'de> for KeySeed<'_> {
    type Value = (String, Option<Position>);

    fn deserialize<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl<'de> Visitor<'de> for KeySeed<'_> {
    type Value = (String, Option<Position>);

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an object key")
    }

    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E> {
        Ok((v.to_owned(), self.ctx.position_of(v)))
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
        Ok((v.to_owned(), None))
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E> {
        Ok((v, None))
    }
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
        Ok(Node::null())
    }

    fn visit_bool<E>(self, v: bool) -> Result<Node, E> {
        Ok(Node::from(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Node, E> {
        Ok(Node::from(Number::from(v)))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Node, E> {
        Ok(Node::from(Number::from(v)))
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Node, E> {
        Number::from_f64(v)
            .map(Node::from)
            .ok_or_else(|| E::custom("non-finite number"))
    }

    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Node, E> {
        Ok(Node::from(v).with_position(self.ctx.position_of(v)))
    }

    fn visit_str<E>(self, v: &str) -> Result<Node, E> {
        Ok(Node::from(v))
    }

    fn visit_string<E>(self, v: String) -> Result<Node, E> {
        Ok(Node::from(v))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Node, A::Error> {
        let child = self.child()?;
        let mut out = Vec::new();
        while let Some(value) = seq.next_element_seed(NodeSeed { ..child })? {
            out.push(value);
        }
        Ok(Node::from(out))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
        let ctx = self.ctx;
        let Some((first, first_position)) = map.next_key_seed(KeySeed { ctx })? else {
            self.child::<A::Error>()?;
            return Ok(Node::from(NodeObject::default()));
        };
        if self.ctx.arbitrary_precision && first == ARBITRARY_PRECISION_TOKEN {
            // serde_json's arbitrary_precision number protocol: one entry holding the number's
            // source text. This is a scalar, so it does not count toward the depth limit.
            let text = map.next_value::<String>()?;
            return parse_number_text(&text).map(Node::from);
        }
        let child = self.child()?;
        let mut out = IndexMap::new();
        let mut key = Some((first, first_position));
        while let Some((k, position)) = key {
            if out.contains_key(&k) {
                return Err(self.ctx.fail(ParseAbort::DuplicateKey(k)));
            }
            let value = map
                .next_value_seed(NodeSeed { ..child })?
                .with_position(position);
            out.insert(k, value);
            key = map.next_key_seed(KeySeed { ctx })?;
        }
        // An object without a key of its own (an array element) is positioned at its first key.
        Ok(Node::from(NodeObject(out)).with_position(first_position))
    }
}

#[cfg(test)]
pub(crate) fn parse_node(json: &str) -> Result<Node, serde_json::Error> {
    let ctx = ParseContext::new(64, json.as_bytes());
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
        assert_eq!(parse_node("1e2").unwrap().to_json_string(), "100.0");
        assert_eq!(parse_node("-5").unwrap().to_json_string(), "-5");
    }

    /// Feeds `entries` to the node visitor as a map, as serde_json does for numbers when its
    /// `arbitrary_precision` feature is unified into the build.
    fn visit_token_map(
        arbitrary_precision: bool,
        entries: &[(&str, &str)],
    ) -> Result<Node, String> {
        let mut ctx = ParseContext::new(64, &[]);
        ctx.arbitrary_precision = arbitrary_precision;
        let map = de::value::MapDeserializer::<_, de::value::Error>::new(
            entries.iter().map(|(k, v)| (k.to_string(), v.to_string())),
        );
        NodeSeed {
            ctx: &ctx,
            depth: 0,
        }
        .visit_map(map)
        .map_err(|e| e.to_string())
    }

    #[test]
    fn arbitrary_precision_numbers_are_numbers() {
        let token = ARBITRARY_PRECISION_TOKEN;
        let number = |text: &str| visit_token_map(true, &[(token, text)]).unwrap();
        // In range integers stay exact; out-of-range integers become f64 like the default build.
        assert_eq!(number("42"), Node::from(42i64));
        assert_eq!(number("-42"), Node::from(-42i64));
        let big = number("18446744073709551616");
        assert!(big.as_number().unwrap().is_float(), "{big:?}");
        assert_eq!(big.to_json_string(), "1.8446744073709552e19");
        assert_eq!(
            number("-9223372036854775809").as_number().unwrap().as_f64(),
            -9.223372036854776e18
        );
        assert_eq!(number("1.5"), parse_node("1.5").unwrap());
        assert_eq!(number("1e2"), parse_node("1e2").unwrap());
        assert_eq!(number("-0.0"), parse_node("0.0").unwrap());
        assert!(visit_token_map(true, &[(token, "1e999")]).is_err());
    }

    #[test]
    fn arbitrary_precision_token_is_an_ordinary_key_when_feature_is_off() {
        let token = ARBITRARY_PRECISION_TOKEN;
        let node = visit_token_map(false, &[(token, "1")]).unwrap();
        assert_eq!(node.as_object().unwrap().get(token), Some(&Node::from("1")));
        // With the feature on, a token key that is not first is still an ordinary key.
        let node = visit_token_map(true, &[("a", "1"), (token, "2")]).unwrap();
        assert_eq!(node.as_object().unwrap().len(), 2);
    }

    #[test]
    fn large_integers_follow_the_f64_policy_in_any_feature_state() {
        // Passes whether or not `arbitrary_precision` is unified into this build.
        let big = parse_node("18446744073709551616").unwrap();
        assert!(big.as_number().unwrap().is_float(), "{big:?}");
        assert!(parse_node("1.5").unwrap().as_number().unwrap().is_float());
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
        let ctx = ParseContext::new(3, b"[[[[1]]]]");
        let mut de = serde_json::Deserializer::from_str("[[[[1]]]]");
        assert!(NodeSeed {
            ctx: &ctx,
            depth: 0
        }
        .deserialize(&mut de)
        .is_err());
        assert_eq!(*ctx.abort.borrow(), Some(ParseAbort::DepthLimit(3)));
        let ctx = ParseContext::new(3, b"[[[1]]]");
        let mut de = serde_json::Deserializer::from_str("[[[1]]]");
        assert!(NodeSeed {
            ctx: &ctx,
            depth: 0
        }
        .deserialize(&mut de)
        .is_ok());
    }

    #[test]
    fn locations_are_assigned_and_ignored_by_equality() {
        let mut node = parse_node(r#"{"a/b":[1,{"c":true}]}"#).unwrap();
        let plain = node.clone();
        node.assign_locations(&Arc::from("m.json"), &mut String::from("/metadata/x"), None);
        let nested = node.pointer("/a~1b/1/c").unwrap();
        assert_eq!(nested.source_location().source(), "m.json");
        assert_eq!(
            nested.source_location().pointer(),
            Some("/metadata/x/a~1b/1/c")
        );
        assert_eq!(node.source_location().pointer(), Some("/metadata/x"));
        assert_eq!(node, plain);
        assert_eq!(hash_of(&node), hash_of(&plain));
        let err = nested.expect_str().unwrap_err();
        assert_eq!(err.source_location(), nested.source_location());
        assert!(plain.source_location().is_none());
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
            b.with_sorted_keys().to_json_string(),
            r#"{"a":1,"b":[1,2]}"#
        );
    }

    #[test]
    fn pointers_and_accessors() {
        let n = parse_node(r#"{"a/b":{"~x":[10,true,null]}}"#).unwrap();
        assert_eq!(n.pointer(""), Some(&n));
        assert_eq!(n.pointer("/a~1b/~0x/1"), Some(&Node::from(true)));
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
            Just(Node::null()),
            any::<bool>().prop_map(Node::from),
            any::<i64>().prop_map(Node::from),
            any::<u64>().prop_map(Node::from),
            (-1e300f64..1e300).prop_map(|f| Node::from(Number::from_f64(f).unwrap())),
            ".{0,8}".prop_map(Node::from),
        ];
        leaf.prop_recursive(4, 32, 6, |inner| {
            prop_oneof![
                prop::collection::vec(inner.clone(), 0..6).prop_map(Node::from),
                prop::collection::vec(("[a-c]{0,2}", inner), 0..6)
                    .prop_map(|entries| Node::from(entries.into_iter().collect::<NodeObject>())),
            ]
        })
    }

    proptest! {
        #[test]
        fn json_round_trip(node in arb_node()) {
            let json = node.to_json_string();
            let back = parse_node(&json).unwrap();
            prop_assert_eq!(&parse_node(&node.to_json_string_pretty()).unwrap(), &node);
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
