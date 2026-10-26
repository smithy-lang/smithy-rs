/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Private Smithy JSON AST parser used by [`ModelAssembler`](crate::ModelAssembler).
//!
//! The document is parsed with a streaming visitor. Each shape is read as a generic
//! [`Node`] (rejecting duplicate keys and excessive nesting) and converted into a typed
//! declaration immediately, so the whole document never exists as one generic tree.
//!
//! # Why not `#[derive(Deserialize)]` or `serde_json::Value`?
//!
//! `serde_json` is used only as a tokenizer. The JSON AST is converted by hand, for these
//! reasons:
//!
//! - **Diagnostics, not first-error failure.** A derived model aborts on the first mismatch
//!   with a generic serde message. Hand conversion keeps going, collecting every unknown
//!   property, missing field, wrong type, and invalid shape ID. Each diagnostic has a stable
//!   [`DiagnosticCode`] and a JSON Pointer to the offending value.
//! - **Duplicate keys.** `serde_json` keeps the last value for a repeated key, in both
//!   `Value` and derived maps. [`NodeSeed`] rejects duplicates at every depth instead.
//! - **Resource limits.** Depth and shape-count limits are enforced while parsing and
//!   reported as `ResourceLimit`, not as a recursion error or a panic.
//! - **Bounded peak memory.** Using `serde_json::Value` would materialize the whole document
//!   (about 6 MiB of JSON for EC2) before conversion. Here only one shape's [`Node`] is alive
//!   at a time, and its trait values move into the declaration without being cloned.
//! - **Explicit unsupported features.** `apply`, `set`, mixins, and `@mixin` declarations
//!   produce `UnsupportedFeature`. They are neither silently accepted nor mixed in with
//!   ordinary unknown-field errors.
//! - **No serde surface.** JSON is converted directly into the crate's owned semantic
//!   [`Shape`] types, the same types callers build in code. Nothing about
//!   the JSON AST encoding leaks into the model or the public API.

use crate::diagnostic::{
    escape_pointer_token, push_pointer_token, Diagnostic, DiagnosticCode, DiagnosticSet, LoadError,
    Position, SourceLocation,
};
use crate::node::{KeySeed, Node, NodeObject, NodeSeed, NodeValue, ParseAbort, ParseContext};
use crate::shape::owned::Common;
use crate::shape::{
    EnumShape, IntEnumShape, ListShape, MapShape, MemberShape, Members, OperationShape,
    ResourceShape, ServiceShape, Shape, ShapeType, SimpleShape, StructureShape, TraitMap,
    UnionShape,
};
use crate::shape_id::ShapeId;
use indexmap::IndexMap;
use serde::de::{self, DeserializeSeed, MapAccess, Visitor};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

pub(crate) const DEFAULT_MAX_INPUT_BYTES: u64 = 256 * 1024 * 1024;
pub(crate) const DEFAULT_MAX_SHAPES: usize = 1_000_000;
/// The deepest document nesting the loader supports.
///
/// Depth counts every object and array from the document root. serde_json's own recursion
/// guard stops at 128; keeping the configured limit at or below this value guarantees the
/// loader's limit fires first, so deep input is always reported as `ResourceLimit` rather than
/// as a JSON syntax error.
pub(crate) const MAX_SUPPORTED_DEPTH: usize = 100;
pub(crate) const DEFAULT_MAX_DEPTH: usize = MAX_SUPPORTED_DEPTH;
/// Far above anything a real model produces, but bounds memory for adversarial input.
pub(crate) const DEFAULT_MAX_DIAGNOSTICS: usize = 1_000;

/// One parsed JSON AST document.
#[derive(Debug, Clone, Default)]
pub(crate) struct Document {
    pub(crate) metadata: NodeObject,
    pub(crate) shapes: IndexMap<ShapeId, Shape>,
}

/// Parser limits, configured through [`ModelAssembler`](crate::ModelAssembler).
#[derive(Debug, Clone)]
pub(crate) struct JsonLoader {
    pub(crate) max_input_bytes: u64,
    pub(crate) max_shapes: usize,
    pub(crate) max_depth: usize,
    pub(crate) max_diagnostics: usize,
}

impl Default for JsonLoader {
    fn default() -> Self {
        Self {
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
            max_shapes: DEFAULT_MAX_SHAPES,
            max_depth: DEFAULT_MAX_DEPTH,
            max_diagnostics: DEFAULT_MAX_DIAGNOSTICS,
        }
    }
}

impl JsonLoader {
    pub(crate) fn parse_str(&self, source: &str, input: &str) -> Result<Document, LoadError> {
        self.parse_slice(source, input.as_bytes())
    }

    pub(crate) fn parse_slice(&self, source: &str, input: &[u8]) -> Result<Document, LoadError> {
        let source: Arc<str> = Arc::from(source);
        if input.len() as u64 > self.max_input_bytes {
            return Err(self.input_too_large(&source));
        }
        self.parse(source, input)
    }

    pub(crate) fn parse_reader(
        &self,
        source: &str,
        reader: impl Read,
    ) -> Result<Document, LoadError> {
        // Buffering is much faster than serde_json's byte-at-a-time reader, and `take`
        // ensures an unbounded reader is never retained beyond the limit.
        let mut buffer = Vec::new();
        if let Err(err) = reader
            .take(self.max_input_bytes.saturating_add(1))
            .read_to_end(&mut buffer)
        {
            return Err(Self::read_error(source, err));
        }
        self.parse_slice(source, &buffer)
    }

    /// The error for a reader that failed.
    pub(crate) fn read_error(source: &str, err: std::io::Error) -> LoadError {
        let mut diagnostics = DiagnosticSet::default();
        diagnostics.push(Diagnostic::error_at(
            DiagnosticCode::Io,
            format!("failed to read model: {err}"),
            Some(SourceLocation::new(Arc::from(source))),
        ));
        LoadError::io(err, diagnostics)
    }

    pub(crate) fn parse_file(&self, path: &Path) -> Result<Document, LoadError> {
        let source = path.display().to_string();
        match std::fs::File::open(path) {
            Ok(file) => self.parse_reader(&source, file),
            Err(err) => {
                let mut diagnostics = DiagnosticSet::default();
                diagnostics.push(Diagnostic::error_at(
                    DiagnosticCode::Io,
                    format!("failed to open model: {err}"),
                    Some(SourceLocation::new(Arc::from(source))),
                ));
                Err(LoadError::io(err, diagnostics))
            }
        }
    }

    fn input_too_large(&self, source: &Arc<str>) -> LoadError {
        let mut diagnostics = DiagnosticSet::default();
        diagnostics.push(Diagnostic::error_at(
            DiagnosticCode::ResourceLimit,
            format!(
                "input exceeds the maximum of {} bytes",
                self.max_input_bytes
            ),
            Some(SourceLocation::new(source.clone())),
        ));
        LoadError::resource_limit(diagnostics)
    }

    fn parse(&self, source: Arc<str>, input: &[u8]) -> Result<Document, LoadError> {
        let mut de = serde_json::Deserializer::from_slice(input);
        let state = State {
            options: self,
            source: source.clone(),
            ctx: ParseContext::new(self.max_depth, input),
            diagnostics: RefCell::new(DiagnosticSet::with_limit(self.max_diagnostics)),
            interner: RefCell::default(),
            version: RefCell::default(),
            document: RefCell::default(),
        };
        let result = DocumentSeed { state: &state }
            .deserialize(&mut de)
            .and_then(|()| de.end());
        let State {
            ctx,
            diagnostics,
            version,
            document,
            ..
        } = state;
        let mut diagnostics = diagnostics.into_inner();

        if let Err(err) = result {
            let location = SourceLocation::new(source).with_line_column(err.line(), err.column());
            let abort = ctx.abort.into_inner();
            let (code, message) = match &abort {
                Some(ParseAbort::DuplicateKey(key)) => (
                    DiagnosticCode::DuplicateKey,
                    format!("duplicate object key `{key}`"),
                ),
                Some(ParseAbort::DepthLimit(_) | ParseAbort::ShapeLimit(_)) => {
                    (DiagnosticCode::ResourceLimit, syntax_message(&err))
                }
                None => (DiagnosticCode::JsonSyntax, syntax_message(&err)),
            };
            diagnostics.push_unbounded(Diagnostic::error_at(code, message, Some(location)));
            return Err(match abort {
                Some(ParseAbort::DepthLimit(_) | ParseAbort::ShapeLimit(_)) => {
                    LoadError::resource_limit(diagnostics)
                }
                _ => LoadError::json(err, diagnostics),
            });
        }

        let root = SourceLocation::with_pointer(&source, String::new(), None);
        let version = version.into_inner();
        let version_location = root.child_at("/smithy", version.as_ref().and_then(Node::position));
        match version.map(Node::into_value) {
            Some(NodeValue::String(v)) if v == "2" || v == "2.0" => {}
            Some(NodeValue::String(v)) => {
                // Other diagnostics may be artifacts of a different grammar; report only this.
                let mut only = DiagnosticSet::default();
                only.push(Diagnostic::error_at(
                    DiagnosticCode::UnsupportedVersion,
                    format!("unsupported Smithy version `{v}`; only `2` and `2.0` are supported"),
                    Some(version_location.clone()),
                ));
                return Err(LoadError::invalid_model(only));
            }
            Some(other) => diagnostics.push(Diagnostic::error_at(
                DiagnosticCode::InvalidProperty,
                format!(
                    "`smithy` must be a string, found {}",
                    Node::new(other).type_name()
                ),
                Some(version_location),
            )),
            None => diagnostics.push(Diagnostic::error_at(
                DiagnosticCode::MissingProperty,
                "missing `smithy` version property",
                Some(root),
            )),
        }

        if diagnostics.has_errors() {
            Err(LoadError::invalid_model(diagnostics))
        } else {
            Ok(document.into_inner())
        }
    }
}

/// serde_json's message without its ` at line L column C` suffix, which the diagnostic's
/// location already records.
fn syntax_message(err: &serde_json::Error) -> String {
    let message = err.to_string();
    let suffix = format!(" at line {} column {}", err.line(), err.column());
    match message.strip_suffix(&suffix) {
        Some(stripped) => stripped.to_owned(),
        None => message,
    }
}

struct State<'o> {
    options: &'o JsonLoader,
    source: Arc<str>,
    ctx: ParseContext<'o>,
    diagnostics: RefCell<DiagnosticSet>,
    interner: RefCell<HashSet<ShapeId>>,
    version: RefCell<Option<Node>>,
    document: RefCell<Document>,
}

impl State<'_> {
    fn at(&self, pointer: String, position: Option<Position>) -> SourceLocation {
        SourceLocation::with_pointer(&self.source, pointer, position)
    }

    fn error(&self, code: DiagnosticCode, message: impl Into<String>, location: SourceLocation) {
        self.diagnostics
            .borrow_mut()
            .push(Diagnostic::error_at(code, message, Some(location)));
    }

    /// Parses and interns an absolute shape ID so repeated targets share one allocation.
    fn intern(&self, value: &str) -> Option<ShapeId> {
        let mut interner = self.interner.borrow_mut();
        if let Some(id) = interner.get(value) {
            return Some(id.clone());
        }
        let id = ShapeId::new(value).ok()?;
        interner.insert(id.clone());
        Some(id)
    }

    fn shape_id_value(&self, node: &Node, location: SourceLocation) -> Option<ShapeId> {
        match node.as_str() {
            Some(value) => {
                let id = self.intern(value);
                if id.is_none() {
                    self.error(
                        DiagnosticCode::InvalidShapeId,
                        format!("`{value}` is not a valid absolute shape ID"),
                        location,
                    );
                }
                id
            }
            None => {
                self.error(
                    DiagnosticCode::InvalidProperty,
                    format!("expected a shape ID string, found {}", node.type_name()),
                    location,
                );
                None
            }
        }
    }

    fn expect_object<'n>(
        &self,
        node: &'n Node,
        what: &str,
        location: &SourceLocation,
    ) -> Option<&'n NodeObject> {
        let object = node.as_object();
        if object.is_none() {
            self.error(
                DiagnosticCode::InvalidProperty,
                format!("{what} must be an object, found {}", node.type_name()),
                location.clone(),
            );
        }
        object
    }

    fn check_properties(
        &self,
        object: &NodeObject,
        allowed: &[&str],
        what: &str,
        location: &SourceLocation,
    ) -> bool {
        let mut ok = true;
        for (key, value) in object.iter() {
            if !allowed.contains(&key) {
                self.error(
                    DiagnosticCode::UnknownProperty,
                    format!("unknown property `{key}` in {what}"),
                    location.child_at(&format!("/{}", escape_pointer_token(key)), value.position()),
                );
                ok = false;
            }
        }
        ok
    }

    /// `{"target": "ns#Name"}`
    fn shape_ref(&self, node: &Node, location: SourceLocation) -> Option<ShapeId> {
        let object = self.expect_object(node, "a shape reference", &location)?;
        let ok = self.check_properties(object, &["target"], "a shape reference", &location);
        let target = match object.get("target") {
            Some(target) => self.shape_id_value(target, target.source_location().clone()),
            None => {
                self.error(
                    DiagnosticCode::MissingProperty,
                    "shape reference is missing `target`",
                    location,
                );
                None
            }
        };
        target.filter(|_| ok)
    }

    fn optional_ref(&self, object: &NodeObject, key: &str) -> Option<Option<ShapeId>> {
        match object.get(key) {
            None => Some(None),
            Some(node) => self
                .shape_ref(node, node.source_location().clone())
                .map(Some),
        }
    }

    fn ref_list(&self, object: &NodeObject, key: &str) -> Option<Vec<ShapeId>> {
        let Some(node) = object.get(key) else {
            return Some(Vec::new());
        };
        let location = node.source_location().clone();
        let Some(elements) = node.as_array() else {
            self.error(
                DiagnosticCode::InvalidProperty,
                format!("`{key}` must be an array, found {}", node.type_name()),
                location,
            );
            return None;
        };
        let mut ok = true;
        let mut refs: Vec<ShapeId> = Vec::with_capacity(elements.len());
        let mut first_index: HashMap<ShapeId, usize> = HashMap::with_capacity(elements.len());
        for (i, element) in elements.iter().enumerate() {
            let element_location = element.source_location().clone();
            let Some(id) = self.shape_ref(element, element_location.clone()) else {
                ok = false;
                continue;
            };
            // These lists are unordered sets in Smithy; a repeated entry is a modeling error,
            // not something to normalize silently.
            if let Some(&first) = first_index.get(&id) {
                self.diagnostics.borrow_mut().push(
                    Diagnostic::error_at(
                        DiagnosticCode::DuplicateReference,
                        format!("`{key}` lists `{id}` more than once"),
                        Some(element_location),
                    )
                    .with_related(elements[first].source_location().clone()),
                );
                ok = false;
                continue;
            }
            first_index.insert(id.clone(), i);
            refs.push(id);
        }
        ok.then_some(refs)
    }

    /// An object of identifier names to shape references (resource identifiers/properties).
    fn ref_map(&self, object: &NodeObject, key: &str) -> Option<IndexMap<String, ShapeId>> {
        let Some(node) = object.get(key) else {
            return Some(IndexMap::new());
        };
        let location = node.source_location().clone();
        let entries = self.expect_object(node, &format!("`{key}`"), &location)?;
        let mut ok = true;
        let mut out = IndexMap::new();
        for (name, value) in entries.iter() {
            let entry_location = value.source_location().clone();
            if !ShapeId::is_valid_identifier(name) {
                self.error(
                    DiagnosticCode::InvalidProperty,
                    format!("`{name}` is not a valid identifier"),
                    entry_location.clone(),
                );
                ok = false;
            }
            match self.shape_ref(value, entry_location) {
                Some(target) => {
                    out.insert(name.to_owned(), target);
                }
                None => ok = false,
            }
        }
        ok.then_some(out)
    }

    fn traits(&self, node: Option<Node>) -> Option<TraitMap> {
        let Some(node) = node else {
            return Some(TraitMap::new());
        };
        let location = node.source_location().clone();
        let type_name = node.type_name();
        let NodeValue::Object(object) = node.into_value() else {
            self.error(
                DiagnosticCode::InvalidProperty,
                format!("`traits` must be an object, found {type_name}"),
                location,
            );
            return None;
        };
        let mut ok = true;
        let mut traits = TraitMap::with_capacity(object.len());
        for (key, value) in object.into_entries() {
            match self.intern(&key).filter(|id| !id.is_member()) {
                Some(id) => {
                    traits.insert(id, value);
                }
                None => {
                    self.error(
                        DiagnosticCode::InvalidShapeId,
                        format!("trait `{key}` is not an absolute root shape ID"),
                        value.source_location().clone(),
                    );
                    ok = false;
                }
            }
        }
        ok.then_some(traits)
    }

    fn member(
        &self,
        container: &ShapeId,
        name: &str,
        node: Node,
        location: SourceLocation,
    ) -> Option<MemberShape> {
        let Ok(id) = container.with_member(name) else {
            self.error(
                DiagnosticCode::InvalidShapeId,
                format!("`{name}` is not a valid member name"),
                location,
            );
            return None;
        };
        let type_name = node.type_name();
        let NodeValue::Object(mut object) = node.into_value() else {
            self.error(
                DiagnosticCode::InvalidProperty,
                format!("member `{name}` must be an object, found {type_name}"),
                location,
            );
            return None;
        };
        let ok = self.check_properties(&object, &["target", "traits"], "a member", &location);
        let target = match object.get("target") {
            Some(target) => self.shape_id_value(target, target.source_location().clone()),
            None => {
                self.error(
                    DiagnosticCode::MissingProperty,
                    format!("member `{name}` is missing `target`"),
                    location.clone(),
                );
                None
            }
        };
        let traits = self.traits(object.remove("traits"));
        if !ok {
            return None;
        }
        Some(MemberShape {
            id,
            target: target?,
            traits: traits?,
            source: location,
        })
    }

    fn required_member(
        &self,
        object: &mut NodeObject,
        key: &str,
        container: &ShapeId,
        location: &SourceLocation,
    ) -> Option<MemberShape> {
        match object.remove(key) {
            Some(node) => {
                let location = node.source_location().clone();
                self.member(container, key, node, location)
            }
            None => {
                self.error(
                    DiagnosticCode::MissingProperty,
                    format!("`{container}` is missing required member `{key}`"),
                    location.clone(),
                );
                None
            }
        }
    }

    fn named_members(&self, object: &mut NodeObject, container: &ShapeId) -> Option<Members> {
        let Some(node) = object.remove("members") else {
            return Some(Members::new());
        };
        let location = node.source_location().clone();
        let type_name = node.type_name();
        let NodeValue::Object(entries) = node.into_value() else {
            self.error(
                DiagnosticCode::InvalidProperty,
                format!("`members` must be an object, found {type_name}"),
                location,
            );
            return None;
        };
        let mut ok = true;
        let mut members = Members::with_capacity(entries.len());
        for (name, value) in entries.into_entries() {
            let member_location = value.source_location().clone();
            match self.member(container, &name, value, member_location) {
                Some(member) => {
                    members.insert(name, member);
                }
                None => ok = false,
            }
        }
        ok.then_some(members)
    }

    fn optional_string(&self, object: &NodeObject, key: &str) -> Option<Option<String>> {
        match object.get(key) {
            None => Some(None),
            Some(node) if node.as_str().is_some() => node.as_str().map(|v| Some(v.to_owned())),
            Some(other) => {
                self.error(
                    DiagnosticCode::InvalidProperty,
                    format!("`{key}` must be a string, found {}", other.type_name()),
                    other.source_location().clone(),
                );
                None
            }
        }
    }

    fn rename(&self, object: &NodeObject) -> Option<IndexMap<ShapeId, String>> {
        let Some(node) = object.get("rename") else {
            return Some(IndexMap::new());
        };
        let location = node.source_location().clone();
        let entries = self.expect_object(node, "`rename`", &location)?;
        let mut ok = true;
        let mut out = IndexMap::new();
        for (key, value) in entries.iter() {
            let entry_location = value.source_location().clone();
            let id = self.intern(key);
            if id.is_none() {
                self.error(
                    DiagnosticCode::InvalidShapeId,
                    format!("rename key `{key}` is not a valid absolute shape ID"),
                    entry_location.clone(),
                );
            }
            let name = value.as_str();
            if name.is_none() {
                self.error(
                    DiagnosticCode::InvalidProperty,
                    format!("rename value must be a string, found {}", value.type_name()),
                    entry_location,
                );
            }
            match (id, name) {
                (Some(id), Some(name)) => {
                    out.insert(id, name.to_owned());
                }
                _ => ok = false,
            }
        }
        ok.then_some(out)
    }

    fn convert_shape(&self, key: &str, mut node: Node) -> Option<Shape> {
        // One location table covers the shape, its members, references, and trait values.
        let mut pointer = String::with_capacity(8 + key.len());
        pointer.push_str("/shapes/");
        push_pointer_token(&mut pointer, key);
        let position = node.position();
        node.assign_locations(&self.source, &mut pointer, position);
        let location = node.source_location().clone();
        let id = match self.intern(key) {
            Some(id) if !id.is_member() => id,
            _ => {
                self.error(
                    DiagnosticCode::InvalidShapeId,
                    format!("shape key `{key}` is not an absolute root shape ID"),
                    location,
                );
                return None;
            }
        };
        let type_name = node.type_name();
        let NodeValue::Object(mut object) = node.into_value() else {
            self.error(
                DiagnosticCode::InvalidProperty,
                format!("shape `{id}` must be an object, found {type_name}"),
                location,
            );
            return None;
        };
        let property = |name: &str| match object.get(name) {
            Some(value) => value.source_location().clone(),
            None => location.child(&format!("/{name}")),
        };
        let shape_type = match object.get("type").map(|t| (t, t.as_str())) {
            Some((_, Some(name))) => {
                match name {
                    "apply" => {
                        self.error(
                            DiagnosticCode::UnsupportedFeature,
                            format!("`apply` to `{id}` is not supported"),
                            property("type"),
                        );
                        return None;
                    }
                    "set" => {
                        self.error(
                        DiagnosticCode::UnsupportedFeature,
                        format!("`{id}` uses the Smithy 1.0 `set` type; use a list with @uniqueItems"),
                        property("type"),
                    );
                        return None;
                    }
                    name => match ShapeType::from_root_name(name) {
                        Some(shape_type) => shape_type,
                        None => {
                            self.error(
                                DiagnosticCode::UnknownShapeType,
                                format!("unknown shape type `{name}` for `{id}`"),
                                property("type"),
                            );
                            return None;
                        }
                    },
                }
            }
            Some((other, None)) => {
                self.error(
                    DiagnosticCode::InvalidProperty,
                    format!("`type` must be a string, found {}", other.type_name()),
                    property("type"),
                );
                return None;
            }
            None => {
                self.error(
                    DiagnosticCode::MissingProperty,
                    format!("shape `{id}` is missing `type`"),
                    location,
                );
                return None;
            }
        };

        let mut ok = true;
        let mut allowed = vec!["type", "traits", "mixins"];
        allowed.extend_from_slice(kind_properties(shape_type));
        ok &= self.check_properties(
            &object,
            &allowed,
            &format!("{shape_type} `{id}`"),
            &location,
        );

        match object.get("mixins").map(|m| (m, m.as_array())) {
            None => {}
            Some((_, Some([]))) => {}
            Some((_, Some(_))) => {
                self.error(
                    DiagnosticCode::UnsupportedFeature,
                    format!("`{id}` uses mixins, which are not supported"),
                    property("mixins"),
                );
                ok = false;
            }
            Some((other, None)) => {
                self.error(
                    DiagnosticCode::InvalidProperty,
                    format!("`mixins` must be an array, found {}", other.type_name()),
                    property("mixins"),
                );
                ok = false;
            }
        }

        let traits = self.traits(object.remove("traits"));
        let traits_ok = traits.is_some();
        let common = Common {
            id: id.clone(),
            traits: traits.unwrap_or_default(),
            source: location.clone(),
        };
        let shape = self.kind(shape_type, common, &mut object, &location);
        if !ok || !traits_ok {
            return None;
        }
        shape
    }

    fn kind(
        &self,
        shape_type: ShapeType,
        common: Common,
        object: &mut NodeObject,
        location: &SourceLocation,
    ) -> Option<Shape> {
        let id = &common.id.clone();
        Some(match shape_type {
            t if t.is_simple() => Shape::Simple(SimpleShape {
                common,
                shape_type: t,
            }),
            ShapeType::List => Shape::List(ListShape {
                member: self.required_member(object, "member", id, location)?,
                common,
            }),
            ShapeType::Map => {
                let key = self.required_member(object, "key", id, location);
                let value = self.required_member(object, "value", id, location);
                Shape::Map(MapShape {
                    common,
                    key: key?,
                    value: value?,
                })
            }
            ShapeType::Structure => Shape::Structure(StructureShape {
                members: self.named_members(object, id)?,
                common,
            }),
            ShapeType::Union => Shape::Union(UnionShape {
                members: self.named_members(object, id)?,
                common,
            }),
            ShapeType::Enum => Shape::Enum(EnumShape {
                members: self.named_members(object, id)?,
                common,
            }),
            ShapeType::IntEnum => Shape::IntEnum(IntEnumShape {
                members: self.named_members(object, id)?,
                common,
            }),
            ShapeType::Service => {
                let version = self.optional_string(object, "version");
                let operations = self.ref_list(object, "operations");
                let resources = self.ref_list(object, "resources");
                let errors = self.ref_list(object, "errors");
                let rename = self.rename(object);
                Shape::Service(ServiceShape {
                    common,
                    version: version?,
                    operations: operations?,
                    resources: resources?,
                    errors: errors?,
                    rename: rename?,
                })
            }
            ShapeType::Operation => {
                let input = self.optional_ref(object, "input");
                let output = self.optional_ref(object, "output");
                let errors = self.ref_list(object, "errors");
                Shape::Operation(OperationShape {
                    common,
                    input: input?,
                    output: output?,
                    errors: errors?,
                })
            }
            ShapeType::Resource => {
                let identifiers = self.ref_map(object, "identifiers");
                let properties = self.ref_map(object, "properties");
                let create = self.optional_ref(object, "create");
                let put = self.optional_ref(object, "put");
                let read = self.optional_ref(object, "read");
                let update = self.optional_ref(object, "update");
                let delete = self.optional_ref(object, "delete");
                let list = self.optional_ref(object, "list");
                let operations = self.ref_list(object, "operations");
                let collection_operations = self.ref_list(object, "collectionOperations");
                let resources = self.ref_list(object, "resources");
                Shape::Resource(ResourceShape {
                    common,
                    identifiers: identifiers?,
                    properties: properties?,
                    create: create?,
                    put: put?,
                    read: read?,
                    update: update?,
                    delete: delete?,
                    list: list?,
                    operations: operations?,
                    collection_operations: collection_operations?,
                    resources: resources?,
                })
            }
            ShapeType::Member => unreachable!("`member` is not a root shape type"),
            _ => unreachable!("every root shape type is handled"),
        })
    }
}

/// Kind-specific JSON AST properties (in addition to `type`, `traits`, and `mixins`).
fn kind_properties(shape_type: ShapeType) -> &'static [&'static str] {
    match shape_type {
        ShapeType::List => &["member"],
        ShapeType::Map => &["key", "value"],
        ShapeType::Structure | ShapeType::Union | ShapeType::Enum | ShapeType::IntEnum => {
            &["members"]
        }
        ShapeType::Service => &["version", "operations", "resources", "errors", "rename"],
        ShapeType::Operation => &["input", "output", "errors"],
        ShapeType::Resource => &[
            "identifiers",
            "properties",
            "create",
            "put",
            "read",
            "update",
            "delete",
            "list",
            "operations",
            "collectionOperations",
            "resources",
        ],
        _ => &[],
    }
}

struct DocumentSeed<'s, 'o> {
    state: &'s State<'o>,
}

impl<'de> DeserializeSeed<'de> for DocumentSeed<'_, '_> {
    type Value = ();

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for DocumentSeed<'_, '_> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a Smithy JSON AST object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let state = self.state;
        let ctx = &state.ctx;
        let mut seen = HashSet::new();
        while let Some((key, position)) = map.next_key_seed(KeySeed { ctx })? {
            if !seen.insert(key.clone()) {
                return Err(ctx.fail(ParseAbort::DuplicateKey(key)));
            }
            match key.as_str() {
                "smithy" => {
                    let value = map.next_value_seed(NodeSeed { ctx, depth: 1 })?;
                    *state.version.borrow_mut() = Some(value.with_position(position));
                }
                "metadata" => {
                    let mut metadata = map.next_value_seed(NodeSeed { ctx, depth: 1 })?;
                    let type_name = metadata.type_name();
                    metadata.assign_locations(
                        &state.source,
                        &mut String::from("/metadata"),
                        position,
                    );
                    match metadata.into_value() {
                        NodeValue::Object(metadata) => {
                            state.document.borrow_mut().metadata = metadata
                        }
                        _ => state.error(
                            DiagnosticCode::InvalidProperty,
                            format!("`metadata` must be an object, found {type_name}"),
                            state.at("/metadata".into(), position),
                        ),
                    }
                }
                "shapes" => map.next_value_seed(ShapesSeed { state })?,
                _ => {
                    map.next_value_seed(NodeSeed { ctx, depth: 1 })?;
                    state.error(
                        DiagnosticCode::UnknownProperty,
                        format!("unknown top-level property `{key}`"),
                        state.at(format!("/{}", escape_pointer_token(&key)), position),
                    );
                }
            }
        }
        Ok(())
    }
}

struct ShapesSeed<'s, 'o> {
    state: &'s State<'o>,
}

impl<'de> DeserializeSeed<'de> for ShapesSeed<'_, '_> {
    type Value = ();

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for ShapesSeed<'_, '_> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an object of shapes")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let state = self.state;
        let ctx = &state.ctx;
        let mut seen = HashSet::new();
        while let Some((key, position)) = map.next_key_seed(KeySeed { ctx })? {
            if seen.contains(&key) {
                return Err(ctx.fail(ParseAbort::DuplicateKey(key)));
            }
            if seen.len() >= state.options.max_shapes {
                return Err(ctx.fail(ParseAbort::ShapeLimit(state.options.max_shapes)));
            }
            let node = map
                .next_value_seed(NodeSeed { ctx, depth: 2 })?
                .with_position(position);
            if let Some(shape) = state.convert_shape(&key, node) {
                state
                    .document
                    .borrow_mut()
                    .shapes
                    .insert(shape.id().clone(), shape);
            }
            seen.insert(key);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<Document, LoadError> {
        JsonLoader::default().parse_str("test.json", json)
    }

    fn loader() -> JsonLoader {
        JsonLoader::default()
    }

    fn codes(err: &LoadError) -> Vec<DiagnosticCode> {
        err.diagnostics().iter().map(|d| d.code()).collect()
    }

    fn pointers(err: &LoadError) -> Vec<String> {
        err.diagnostics()
            .iter()
            .filter_map(|d| d.source_location()?.pointer().map(str::to_owned))
            .collect()
    }

    fn doc(shapes: &str) -> String {
        format!(r#"{{"smithy":"2.0","shapes":{{{shapes}}}}}"#)
    }

    #[test]
    fn versions() {
        assert!(parse(r#"{"smithy":"2.0"}"#).is_ok());
        assert!(parse(r#"{"smithy":"2"}"#).is_ok());
        let err = parse(r#"{"smithy":"1.0","shapes":{"a#B":{"type":"nope"}}}"#).unwrap_err();
        assert_eq!(codes(&err), [DiagnosticCode::UnsupportedVersion]);
        assert!(err.is_invalid_model());
        let err = parse(r#"{"shapes":{}}"#).unwrap_err();
        assert_eq!(codes(&err), [DiagnosticCode::MissingProperty]);
        let err = parse(r#"{"smithy":2}"#).unwrap_err();
        assert_eq!(codes(&err), [DiagnosticCode::InvalidProperty]);
    }

    #[test]
    fn duplicate_reference_set_entries() {
        let cases = [
            ("service", "operations"),
            ("service", "resources"),
            ("service", "errors"),
            ("operation", "errors"),
            ("resource", "operations"),
            ("resource", "collectionOperations"),
            ("resource", "resources"),
        ];
        for (kind, key) in cases {
            let json = doc(&format!(
                r#""a#S":{{"type":"{kind}","{key}":[{{"target":"a#X"}},{{"target":"a#Y"}},{{"target":"a#X"}}]}}"#
            ));
            let err = parse(&json).unwrap_err();
            assert!(err.is_invalid_model(), "{kind} {key}");
            assert_eq!(
                codes(&err),
                [DiagnosticCode::DuplicateReference],
                "{kind} {key}"
            );
            assert_eq!(pointers(&err), [format!("/shapes/a#S/{key}/2")]);
            let diagnostic = err.diagnostics().iter().next().unwrap();
            let related: Vec<_> = diagnostic
                .related()
                .iter()
                .filter_map(|l| l.pointer())
                .collect();
            assert_eq!(related, [format!("/shapes/a#S/{key}/0")]);
        }
        // Distinct entries are fine.
        let json =
            doc(r#""a#S":{"type":"operation","errors":[{"target":"a#X"},{"target":"a#Y"}]}"#);
        assert!(parse(&json).is_ok());
    }

    #[test]
    fn duplicate_keys() {
        for json in [
            r#"{"smithy":"2.0","smithy":"2.0"}"#.to_string(),
            doc(r#""a#A":{"type":"string"},"a#A":{"type":"string"}"#),
            doc(r#""a#A":{"type":"string","traits":{"a#t":{"x":{"y":1,"y":2}}}}"#),
            doc(
                r#""a#A":{"type":"structure","members":{"m":{"target":"a#A"},"m":{"target":"a#A"}}}"#,
            ),
        ] {
            let err = parse(&json).unwrap_err();
            assert!(err.is_json_syntax(), "{json}");
            assert_eq!(
                codes(&err).last(),
                Some(&DiagnosticCode::DuplicateKey),
                "{json}"
            );
            let location = err
                .diagnostics()
                .iter()
                .last()
                .unwrap()
                .source_location()
                .unwrap();
            assert!(location.line().is_some() && location.column().is_some());
        }
    }

    #[test]
    fn syntax_errors_have_line_and_column() {
        let err = parse("{\n  \"smithy\": \"2.0\",\n  oops\n}").unwrap_err();
        assert!(err.is_json_syntax());
        assert_eq!(codes(&err), [DiagnosticCode::JsonSyntax]);
        let location = err
            .diagnostics()
            .iter()
            .next()
            .unwrap()
            .source_location()
            .unwrap();
        assert_eq!(location.line(), Some(3));
        use std::error::Error as _;
        assert!(err.source().unwrap().is::<serde_json::Error>());
        assert!(parse(r#"{"smithy":"2.0"} x"#).unwrap_err().is_json_syntax());
        assert!(parse("[]").unwrap_err().is_json_syntax());
    }

    #[test]
    fn metadata_values_have_locations() {
        let document = parse(r#"{"smithy":"2.0","metadata":{"a/b":[{"c":1}]}}"#).unwrap();
        let value = document.metadata.get("a/b").unwrap();
        assert_eq!(value.source_location().pointer(), Some("/metadata/a~1b"));
        assert_eq!(
            value.pointer("/0/c").unwrap().source_location().pointer(),
            Some("/metadata/a~1b/0/c")
        );
    }

    #[test]
    fn unsupported_features() {
        for (shapes, pointer) in [
            (
                r#""a#A":{"type":"apply","traits":{"a#t":{}}}"#,
                "/shapes/a#A/type",
            ),
            (
                r#""a#A":{"type":"structure","mixins":[{"target":"a#M"}]}"#,
                "/shapes/a#A/mixins",
            ),
            (
                r#""a#S":{"type":"set","member":{"target":"a#A"}}"#,
                "/shapes/a#S/type",
            ),
        ] {
            let err = parse(&doc(shapes)).unwrap_err();
            assert_eq!(
                codes(&err),
                [DiagnosticCode::UnsupportedFeature],
                "{shapes}"
            );
            assert_eq!(pointers(&err), [pointer]);
        }
        let ok = parse(&doc(r#""a#A":{"type":"structure","mixins":[]}"#)).unwrap();
        assert_eq!(ok.shapes.len(), 1);
    }

    #[test]
    fn unknown_properties_and_types() {
        let err = parse(&doc(
            r#""a#A":{"type":"structure","foo":1,"members":{"m":{"target":"a#A","bar":2}}},
               "a#L":{"type":"list","member":{"target":"a#A"},"key":{"target":"a#A"}},
               "a#X":{"type":"widget"},
               "a#R":{"type":"operation","input":{"target":"a#A","extra":1}}"#,
        ))
        .unwrap_err();
        assert_eq!(
            pointers(&err),
            [
                "/shapes/a#A/foo",
                "/shapes/a#A/members/m/bar",
                "/shapes/a#L/key",
                "/shapes/a#X/type",
                "/shapes/a#R/input/extra"
            ]
        );
        assert_eq!(codes(&err)[3], DiagnosticCode::UnknownShapeType);
        let err = parse(r#"{"smithy":"2.0","extra":{}}"#).unwrap_err();
        assert_eq!(codes(&err), [DiagnosticCode::UnknownProperty]);
    }

    #[test]
    fn invalid_ids_and_missing_properties() {
        let err = parse(&doc(r#""B":{"type":"string"},
               "a#B$c":{"type":"string"},
               "a#C":{"type":"structure","members":{"1x":{"target":"a#B"}}},
               "a#D":{"type":"list","member":{"target":"Relative"}},
               "a#E":{"type":"list"},
               "a#F":{"type":"map","key":{"target":"a#B"}},
               "a#G":{"type":"string","traits":{"relative":{}}},
               "a#H":{},
               "a#I":{"type":"structure","members":{"m":{}}},
               "a#J":"string""#))
        .unwrap_err();
        use DiagnosticCode::*;
        assert_eq!(
            codes(&err),
            [
                InvalidShapeId,
                InvalidShapeId,
                InvalidShapeId,
                InvalidShapeId,
                MissingProperty,
                MissingProperty,
                InvalidShapeId,
                MissingProperty,
                MissingProperty,
                InvalidProperty
            ]
        );
        assert_eq!(pointers(&err)[3], "/shapes/a#D/member/target");
    }

    #[test]
    fn resource_limits() {
        let deep = format!(
            r#"{{"smithy":"2.0","metadata":{{"x":{}1{}}}}}"#,
            "[".repeat(200),
            "]".repeat(200)
        );
        let err = parse(&deep).unwrap_err();
        assert!(err.is_resource_limit());
        assert_eq!(codes(&err), [DiagnosticCode::ResourceLimit]);

        let two = doc(r#""a#A":{"type":"string"},"a#B":{"type":"string"}"#);
        let loader = JsonLoader {
            max_shapes: 1,
            ..loader()
        };
        assert!(loader.parse_str("t", &two).unwrap_err().is_resource_limit());
        let loader = JsonLoader {
            max_shapes: 2,
            ..JsonLoader::default()
        };
        assert!(loader.parse_str("t", &two).is_ok());

        let loader = JsonLoader {
            max_input_bytes: 10,
            ..JsonLoader::default()
        };
        assert!(loader.parse_str("t", &two).unwrap_err().is_resource_limit());
        assert!(loader
            .parse_reader("t", two.as_bytes())
            .unwrap_err()
            .is_resource_limit());
        let exact = JsonLoader {
            max_input_bytes: two.len() as u64,
            ..JsonLoader::default()
        };
        assert!(exact.parse_reader("t", two.as_bytes()).is_ok());
    }

    #[test]
    fn depth_limit_is_always_a_resource_limit() {
        // Depth counts every container from the root: the document and `metadata` objects are
        // two levels, so `arrays` nested arrays reach a total depth of `arrays + 2`.
        let metadata = |arrays: usize| {
            format!(
                r#"{{"smithy":"2.0","metadata":{{"x":{}0{}}}}}"#,
                "[".repeat(arrays),
                "]".repeat(arrays)
            )
        };
        let member_trait = |arrays: usize| {
            doc(&format!(
                r#""a#S":{{"type":"structure","members":{{"m":{{"target":"a#S","traits":{{"a#t":{}0{}}}}}}}}}"#,
                "[".repeat(arrays),
                "]".repeat(arrays)
            ))
        };
        assert!(parse(&metadata(MAX_SUPPORTED_DEPTH - 2)).is_ok());
        assert!(parse(&metadata(MAX_SUPPORTED_DEPTH - 1))
            .unwrap_err()
            .is_resource_limit());
        // The assembler clamps configured depths to `MAX_SUPPORTED_DEPTH`, so serde_json's
        // recursion guard (128) never fires first and misreports deep input as JSON syntax.
        for input in [metadata(200), member_trait(126), member_trait(200)] {
            let err = loader().parse_str("t", &input).unwrap_err();
            assert!(err.is_resource_limit(), "{err}");
            assert_eq!(codes(&err), [DiagnosticCode::ResourceLimit]);
        }
        let shallow = JsonLoader {
            max_depth: 4,
            ..JsonLoader::default()
        };
        assert!(shallow.parse_str("t", &metadata(2)).is_ok());
        assert!(shallow
            .parse_str("t", &metadata(3))
            .unwrap_err()
            .is_resource_limit());
    }

    #[test]
    fn diagnostics_are_bounded() {
        // Each shape has an invalid ID, producing one diagnostic per shape.
        let shapes: Vec<_> = (0..50)
            .map(|i| format!(r#""bad{i}":{{"type":"string"}}"#))
            .collect();
        let json = doc(&shapes.join(","));
        let err = parse(&json).unwrap_err();
        assert_eq!(err.diagnostics().len(), 50);
        assert!(!err.diagnostics().is_truncated());

        let err = JsonLoader {
            max_diagnostics: 10,
            ..JsonLoader::default()
        }
        .parse_str("t", &json)
        .unwrap_err();
        let diagnostics = err.diagnostics();
        assert_eq!(diagnostics.len(), 11);
        assert!(diagnostics.is_truncated());
        assert_eq!(diagnostics.omitted(), 40);
        assert_eq!(
            diagnostics.iter().last().unwrap().code(),
            DiagnosticCode::ResourceLimit
        );
        assert!(err.is_invalid_model());

        // Even a zero limit reports why loading failed.
        let err = JsonLoader {
            max_diagnostics: 0,
            ..JsonLoader::default()
        }
        .parse_str("t", &json)
        .unwrap_err();
        assert_eq!(codes(&err), [DiagnosticCode::ResourceLimit]);
    }

    #[test]
    fn io_errors() {
        struct Failing;
        impl Read for Failing {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("boom"))
            }
        }
        let err = loader().parse_reader("r", Failing).unwrap_err();
        assert!(err.is_io());
        assert_eq!(codes(&err), [DiagnosticCode::Io]);
        let err = loader()
            .parse_file(Path::new("/definitely/not/here.json"))
            .unwrap_err();
        assert!(err.is_io());
    }

    #[test]
    fn every_shape_kind() {
        let json = doc(r#""a#Blob":{"type":"blob","traits":{"a#t":[1,{"b":null}]}},
               "a#L":{"type":"list","member":{"target":"a#Blob","traits":{"a#t":true}}},
               "a#M":{"type":"map","key":{"target":"smithy.api#String"},"value":{"target":"a#L"}},
               "a#S":{"type":"structure","members":{"z":{"target":"a#L"},"a":{"target":"a#M"}}},
               "a#U":{"type":"union","members":{"x":{"target":"a#S"}}},
               "a#E":{"type":"enum","members":{"X":{"target":"smithy.api#Unit"}}},
               "a#I":{"type":"intEnum","members":{"X":{"target":"smithy.api#Unit","traits":{"smithy.api#enumValue":1}}}},
               "a#Svc":{"type":"service","version":"1","operations":[{"target":"a#Op"}],
                        "resources":[{"target":"a#R"}],"errors":[{"target":"a#S"}],
                        "rename":{"a#S":"Renamed"}},
               "a#Op":{"type":"operation","input":{"target":"a#S"},"errors":[{"target":"a#S"}]},
               "a#R":{"type":"resource","identifiers":{"id":{"target":"smithy.api#String"}},
                      "properties":{"p":{"target":"a#L"}},"read":{"target":"a#Op"},
                      "operations":[{"target":"a#Op"}],"collectionOperations":[],
                      "resources":[]}"#);
        let document = parse(&json).unwrap();
        let types: Vec<_> = document.shapes.values().map(|s| s.shape_type()).collect();
        use ShapeType::*;
        assert_eq!(
            types,
            [Blob, List, Map, Structure, Union, Enum, IntEnum, Service, Operation, Resource]
        );
        let structure = &document.shapes["a#S"];
        let names: Vec<_> = structure.members().map(|m| m.name()).collect();
        assert_eq!(names, ["z", "a"]);
        assert_eq!(structure.member("a").unwrap().id(), "a#S$a");
        let map = &document.shapes["a#M"];
        let names: Vec<_> = map.members().map(|m| m.id().as_str()).collect();
        assert_eq!(names, ["a#M$key", "a#M$value"]);
        let op = document.shapes["a#Op"].as_operation().unwrap();
        assert_eq!(op.declared_input().unwrap(), "a#S");
        assert!(op.declared_output().is_none());
        let service = document.shapes["a#Svc"].as_service().unwrap();
        assert_eq!(service.rename["a#S"], "Renamed");
        let resource = document.shapes["a#R"].as_resource().unwrap();
        assert_eq!(resource.identifiers["id"], "smithy.api#String");
        assert_eq!(resource.read().unwrap(), "a#Op");
        let trait_value = &document.shapes["a#Blob"].common().traits["a#t"];
        let nested = trait_value.pointer("/1/b").unwrap();
        assert!(nested.is_null());
        // Every trait value records its own JSON Pointer.
        assert_eq!(
            nested.source_location().pointer(),
            Some("/shapes/a#Blob/traits/a#t/1/b")
        );
        assert_eq!(nested.source_location().source(), "test.json");
        // Repeated targets share one allocation.
        let a = &document.shapes["a#S"].member("z").unwrap().target;
        let b = &document.shapes["a#M"].member("value").unwrap().target;
        assert!(a.shares_allocation(b));
    }
}
