/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Smithy JSON AST loading.
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
//! - **No public serde surface.** The declaration types in `ast` are crate-private, so their
//!   representation can change, for example when assembly and mixin support land, without
//!   breaking callers or committing to a serde data format.

use crate::ast::{
    DeclKind, Document, MemberDecl, Members, OperationDecl, ResourceDecl, ServiceDecl, ShapeDecl,
    TraitMap,
};
use crate::diagnostic::{
    escape_pointer_token, Diagnostic, DiagnosticCode, DiagnosticSet, LoadError, SourceLocation,
};
use crate::node::{Node, NodeObject, NodeSeed, ParseAbort, ParseContext};
use crate::shape::ShapeType;
use crate::shape_id::ShapeId;
use indexmap::IndexMap;
use serde::de::{self, DeserializeSeed, MapAccess, Visitor};
use std::cell::RefCell;
use std::collections::HashSet;
use std::fmt;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

pub(crate) const PRELUDE_NAMESPACE: &str = "smithy.api";
pub(crate) const MIXIN_TRAIT: &str = "smithy.api#mixin";

const DEFAULT_MAX_INPUT_BYTES: u64 = 256 * 1024 * 1024;
const DEFAULT_MAX_SHAPES: usize = 1_000_000;
// Below serde_json's own recursion limit (128) so the configured limit is what fires.
const DEFAULT_MAX_DEPTH: usize = 100;

/// Options for loading a model.
///
/// The defaults inject the Smithy prelude, allow applied traits whose definitions are
/// absent, and use resource limits far above the largest AWS models.
#[derive(Debug, Clone)]
pub struct ModelLoader {
    pub(crate) prelude: bool,
    pub(crate) require_trait_definitions: bool,
    pub(crate) max_input_bytes: u64,
    pub(crate) max_shapes: usize,
    pub(crate) max_depth: usize,
}

impl Default for ModelLoader {
    fn default() -> Self {
        Self {
            prelude: true,
            require_trait_definitions: false,
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
            max_shapes: DEFAULT_MAX_SHAPES,
            max_depth: DEFAULT_MAX_DEPTH,
        }
    }
}

impl ModelLoader {
    /// Creates a loader with default options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Does not inject the prelude. References to `smithy.api` shapes then fail to resolve
    /// unless the document defines them.
    pub fn disable_prelude(mut self) -> Self {
        self.prelude = false;
        self
    }

    /// Requires every applied trait to resolve to a trait definition.
    pub fn require_trait_definitions(mut self, require: bool) -> Self {
        self.require_trait_definitions = require;
        self
    }

    /// The maximum number of input bytes.
    pub fn max_input_bytes(mut self, max: u64) -> Self {
        self.max_input_bytes = max;
        self
    }

    /// The maximum number of top-level shapes in a document.
    pub fn max_shapes(mut self, max: usize) -> Self {
        self.max_shapes = max;
        self
    }

    /// The maximum JSON nesting depth.
    pub fn max_depth(mut self, max: usize) -> Self {
        self.max_depth = max;
        self
    }

    pub(crate) fn parse_str(&self, source: &str, input: &str) -> Result<Document, LoadError> {
        self.parse_slice(source, input.as_bytes())
    }

    pub(crate) fn parse_slice(&self, source: &str, input: &[u8]) -> Result<Document, LoadError> {
        let source: Arc<str> = Arc::from(source);
        if input.len() as u64 > self.max_input_bytes {
            return Err(self.input_too_large(&source));
        }
        self.parse(source, serde_json::Deserializer::from_slice(input))
    }

    pub(crate) fn parse_reader(
        &self,
        source: &str,
        reader: impl Read,
    ) -> Result<Document, LoadError> {
        let source_arc: Arc<str> = Arc::from(source);
        // Buffering is much faster than serde_json's byte-at-a-time reader, and `take`
        // ensures an unbounded reader is never retained beyond the limit.
        let mut buffer = Vec::new();
        if let Err(err) = reader
            .take(self.max_input_bytes.saturating_add(1))
            .read_to_end(&mut buffer)
        {
            let mut diagnostics = DiagnosticSet::default();
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::Io,
                format!("failed to read model: {err}"),
                Some(SourceLocation::new(source_arc)),
            ));
            return Err(LoadError::io(err, diagnostics));
        }
        self.parse_slice(source, &buffer)
    }

    pub(crate) fn parse_file(&self, path: &Path) -> Result<Document, LoadError> {
        let source = path.display().to_string();
        match std::fs::File::open(path) {
            Ok(file) => self.parse_reader(&source, file),
            Err(err) => {
                let mut diagnostics = DiagnosticSet::default();
                diagnostics.push(Diagnostic::error(
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
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::ResourceLimit,
            format!(
                "input exceeds the maximum of {} bytes",
                self.max_input_bytes
            ),
            Some(SourceLocation::new(source.clone())),
        ));
        LoadError::resource_limit(diagnostics)
    }

    fn parse<'de, R: serde_json::de::Read<'de>>(
        &self,
        source: Arc<str>,
        mut de: serde_json::Deserializer<R>,
    ) -> Result<Document, LoadError> {
        let state = State {
            options: self,
            source: source.clone(),
            ctx: ParseContext::new(self.max_depth),
            diagnostics: RefCell::default(),
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
                    (DiagnosticCode::ResourceLimit, err.to_string())
                }
                None => (DiagnosticCode::JsonSyntax, err.to_string()),
            };
            diagnostics.push(Diagnostic::error(code, message, Some(location)));
            return Err(match abort {
                Some(ParseAbort::DepthLimit(_) | ParseAbort::ShapeLimit(_)) => {
                    LoadError::resource_limit(diagnostics)
                }
                _ => LoadError::json(err, diagnostics),
            });
        }

        let root = SourceLocation::with_pointer(&source, String::new());
        match version.into_inner() {
            Some(Node::String(v)) if v == "2" || v == "2.0" => {}
            Some(Node::String(v)) => {
                // Other diagnostics may be artifacts of a different grammar; report only this.
                let mut only = DiagnosticSet::default();
                only.push(Diagnostic::error(
                    DiagnosticCode::UnsupportedVersion,
                    format!("unsupported Smithy version `{v}`; only `2` and `2.0` are supported"),
                    Some(root.child("/smithy")),
                ));
                return Err(LoadError::invalid_model(only));
            }
            Some(other) => diagnostics.push(Diagnostic::error(
                DiagnosticCode::InvalidProperty,
                format!("`smithy` must be a string, found {}", other.type_name()),
                Some(root.child("/smithy")),
            )),
            None => diagnostics.push(Diagnostic::error(
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

struct State<'o> {
    options: &'o ModelLoader,
    source: Arc<str>,
    ctx: ParseContext,
    diagnostics: RefCell<DiagnosticSet>,
    interner: RefCell<HashSet<ShapeId>>,
    version: RefCell<Option<Node>>,
    document: RefCell<Document>,
}

impl State<'_> {
    fn at(&self, pointer: String) -> SourceLocation {
        SourceLocation::with_pointer(&self.source, pointer)
    }

    fn error(&self, code: DiagnosticCode, message: impl Into<String>, location: SourceLocation) {
        self.diagnostics
            .borrow_mut()
            .push(Diagnostic::error(code, message, Some(location)));
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
        match node {
            Node::String(value) => {
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
            other => {
                self.error(
                    DiagnosticCode::InvalidProperty,
                    format!("expected a shape ID string, found {}", other.type_name()),
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
        for key in object.keys() {
            if !allowed.contains(&key) {
                self.error(
                    DiagnosticCode::UnknownProperty,
                    format!("unknown property `{key}` in {what}"),
                    location.child(&format!("/{}", escape_pointer_token(key))),
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
            Some(target) => self.shape_id_value(target, location.child("/target")),
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

    fn optional_ref(
        &self,
        object: &NodeObject,
        key: &str,
        location: &SourceLocation,
    ) -> Option<Option<ShapeId>> {
        match object.get(key) {
            None => Some(None),
            Some(node) => self
                .shape_ref(node, location.child(&format!("/{key}")))
                .map(Some),
        }
    }

    fn ref_list(
        &self,
        object: &NodeObject,
        key: &str,
        location: &SourceLocation,
    ) -> Option<Vec<ShapeId>> {
        let Some(node) = object.get(key) else {
            return Some(Vec::new());
        };
        let location = location.child(&format!("/{key}"));
        let Some(elements) = node.as_array() else {
            self.error(
                DiagnosticCode::InvalidProperty,
                format!("`{key}` must be an array, found {}", node.type_name()),
                location,
            );
            return None;
        };
        let refs: Vec<_> = elements
            .iter()
            .enumerate()
            .map(|(i, element)| self.shape_ref(element, location.child(&format!("/{i}"))))
            .collect();
        refs.into_iter().collect()
    }

    /// An object of identifier names to shape references (resource identifiers/properties).
    fn ref_map(
        &self,
        object: &NodeObject,
        key: &str,
        location: &SourceLocation,
    ) -> Option<IndexMap<String, ShapeId>> {
        let Some(node) = object.get(key) else {
            return Some(IndexMap::new());
        };
        let location = location.child(&format!("/{key}"));
        let entries = self.expect_object(node, &format!("`{key}`"), &location)?;
        let mut ok = true;
        let mut out = IndexMap::new();
        for (name, value) in entries.iter() {
            let entry_location = location.child(&format!("/{}", escape_pointer_token(name)));
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

    fn traits(&self, node: Option<Node>, location: &SourceLocation) -> Option<TraitMap> {
        let Some(node) = node else {
            return Some(TraitMap::new());
        };
        let location = location.child("/traits");
        let Node::Object(object) = node else {
            self.error(
                DiagnosticCode::InvalidProperty,
                format!("`traits` must be an object, found {}", node.type_name()),
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
                        location.child(&format!("/{}", escape_pointer_token(&key))),
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
    ) -> Option<MemberDecl> {
        let Ok(id) = container.with_member(name) else {
            self.error(
                DiagnosticCode::InvalidShapeId,
                format!("`{name}` is not a valid member name"),
                location,
            );
            return None;
        };
        let Node::Object(mut object) = node else {
            self.error(
                DiagnosticCode::InvalidProperty,
                format!(
                    "member `{name}` must be an object, found {}",
                    node.type_name()
                ),
                location,
            );
            return None;
        };
        let ok = self.check_properties(&object, &["target", "traits"], "a member", &location);
        let target = match object.get("target") {
            Some(target) => self.shape_id_value(target, location.child("/target")),
            None => {
                self.error(
                    DiagnosticCode::MissingProperty,
                    format!("member `{name}` is missing `target`"),
                    location.clone(),
                );
                None
            }
        };
        let traits = self.traits(object.remove("traits"), &location);
        if !ok {
            return None;
        }
        Some(MemberDecl {
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
    ) -> Option<MemberDecl> {
        match object.remove(key) {
            Some(node) => self.member(container, key, node, location.child(&format!("/{key}"))),
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

    fn named_members(
        &self,
        object: &mut NodeObject,
        container: &ShapeId,
        location: &SourceLocation,
    ) -> Option<Members> {
        let Some(node) = object.remove("members") else {
            return Some(Members::new());
        };
        let location = location.child("/members");
        let Node::Object(entries) = node else {
            self.error(
                DiagnosticCode::InvalidProperty,
                format!("`members` must be an object, found {}", node.type_name()),
                location,
            );
            return None;
        };
        let mut ok = true;
        let mut members = Members::with_capacity(entries.len());
        for (name, value) in entries.into_entries() {
            let member_location = location.child(&format!("/{}", escape_pointer_token(&name)));
            match self.member(container, &name, value, member_location) {
                Some(member) => {
                    members.insert(name, member);
                }
                None => ok = false,
            }
        }
        ok.then_some(members)
    }

    fn optional_string(
        &self,
        object: &NodeObject,
        key: &str,
        location: &SourceLocation,
    ) -> Option<Option<String>> {
        match object.get(key) {
            None => Some(None),
            Some(Node::String(value)) => Some(Some(value.clone())),
            Some(other) => {
                self.error(
                    DiagnosticCode::InvalidProperty,
                    format!("`{key}` must be a string, found {}", other.type_name()),
                    location.child(&format!("/{key}")),
                );
                None
            }
        }
    }

    fn rename(
        &self,
        object: &NodeObject,
        location: &SourceLocation,
    ) -> Option<IndexMap<ShapeId, String>> {
        let Some(node) = object.get("rename") else {
            return Some(IndexMap::new());
        };
        let location = location.child("/rename");
        let entries = self.expect_object(node, "`rename`", &location)?;
        let mut ok = true;
        let mut out = IndexMap::new();
        for (key, value) in entries.iter() {
            let entry_location = location.child(&format!("/{}", escape_pointer_token(key)));
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

    fn convert_shape(&self, key: &str, node: Node) -> Option<ShapeDecl> {
        let location = self.at(format!("/shapes/{}", escape_pointer_token(key)));
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
        let Node::Object(mut object) = node else {
            self.error(
                DiagnosticCode::InvalidProperty,
                format!("shape `{id}` must be an object, found {}", node.type_name()),
                location,
            );
            return None;
        };
        let shape_type = match object.get("type") {
            Some(Node::String(name)) => {
                match name.as_str() {
                    "apply" => {
                        self.error(
                            DiagnosticCode::UnsupportedFeature,
                            format!("`apply` to `{id}` is not supported"),
                            location.child("/type"),
                        );
                        return None;
                    }
                    "set" => {
                        self.error(
                        DiagnosticCode::UnsupportedFeature,
                        format!("`{id}` uses the Smithy 1.0 `set` type; use a list with @uniqueItems"),
                        location.child("/type"),
                    );
                        return None;
                    }
                    name => match ShapeType::from_root_name(name) {
                        Some(shape_type) => shape_type,
                        None => {
                            self.error(
                                DiagnosticCode::UnknownShapeType,
                                format!("unknown shape type `{name}` for `{id}`"),
                                location.child("/type"),
                            );
                            return None;
                        }
                    },
                }
            }
            Some(other) => {
                self.error(
                    DiagnosticCode::InvalidProperty,
                    format!("`type` must be a string, found {}", other.type_name()),
                    location.child("/type"),
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
        if self.options.prelude && id.namespace() == PRELUDE_NAMESPACE {
            self.error(
                DiagnosticCode::PreludeConflict,
                format!("`{id}` cannot be defined in the prelude namespace `{PRELUDE_NAMESPACE}`"),
                location.clone(),
            );
            ok = false;
        }

        let mut allowed = vec!["type", "traits", "mixins"];
        allowed.extend_from_slice(kind_properties(shape_type));
        ok &= self.check_properties(
            &object,
            &allowed,
            &format!("{shape_type} `{id}`"),
            &location,
        );

        match object.get("mixins") {
            None => {}
            Some(Node::Array(mixins)) if mixins.is_empty() => {}
            Some(Node::Array(_)) => {
                self.error(
                    DiagnosticCode::UnsupportedFeature,
                    format!("`{id}` uses mixins, which are not supported"),
                    location.child("/mixins"),
                );
                ok = false;
            }
            Some(other) => {
                self.error(
                    DiagnosticCode::InvalidProperty,
                    format!("`mixins` must be an array, found {}", other.type_name()),
                    location.child("/mixins"),
                );
                ok = false;
            }
        }

        let traits = self.traits(object.remove("traits"), &location);
        if traits.as_ref().is_some_and(|t| t.contains_key(MIXIN_TRAIT)) {
            self.error(
                DiagnosticCode::UnsupportedFeature,
                format!("`{id}` is a mixin (`{MIXIN_TRAIT}`), which is not supported"),
                location.child(&format!("/traits/{MIXIN_TRAIT}")),
            );
            ok = false;
        }

        let kind = self.kind(shape_type, &id, &mut object, &location);
        if !ok {
            return None;
        }
        Some(ShapeDecl {
            id,
            kind: kind?,
            traits: traits?,
            source: location,
        })
    }

    fn kind(
        &self,
        shape_type: ShapeType,
        id: &ShapeId,
        object: &mut NodeObject,
        location: &SourceLocation,
    ) -> Option<DeclKind> {
        Some(match shape_type {
            t if t.is_simple() => DeclKind::Simple(t),
            ShapeType::List => DeclKind::List {
                member: self.required_member(object, "member", id, location)?,
            },
            ShapeType::Map => {
                let key = self.required_member(object, "key", id, location);
                let value = self.required_member(object, "value", id, location);
                DeclKind::Map {
                    key: key?,
                    value: value?,
                }
            }
            ShapeType::Structure => DeclKind::Structure(self.named_members(object, id, location)?),
            ShapeType::Union => DeclKind::Union(self.named_members(object, id, location)?),
            ShapeType::Enum => DeclKind::Enum(self.named_members(object, id, location)?),
            ShapeType::IntEnum => DeclKind::IntEnum(self.named_members(object, id, location)?),
            ShapeType::Service => {
                let version = self.optional_string(object, "version", location);
                let operations = self.ref_list(object, "operations", location);
                let resources = self.ref_list(object, "resources", location);
                let errors = self.ref_list(object, "errors", location);
                let rename = self.rename(object, location);
                DeclKind::Service(ServiceDecl {
                    version: version?,
                    operations: operations?,
                    resources: resources?,
                    errors: errors?,
                    rename: rename?,
                })
            }
            ShapeType::Operation => {
                let input = self.optional_ref(object, "input", location);
                let output = self.optional_ref(object, "output", location);
                let errors = self.ref_list(object, "errors", location);
                DeclKind::Operation(OperationDecl {
                    input: input?,
                    output: output?,
                    errors: errors?,
                })
            }
            ShapeType::Resource => {
                let identifiers = self.ref_map(object, "identifiers", location);
                let properties = self.ref_map(object, "properties", location);
                let create = self.optional_ref(object, "create", location);
                let put = self.optional_ref(object, "put", location);
                let read = self.optional_ref(object, "read", location);
                let update = self.optional_ref(object, "update", location);
                let delete = self.optional_ref(object, "delete", location);
                let list = self.optional_ref(object, "list", location);
                let operations = self.ref_list(object, "operations", location);
                let collection_operations = self.ref_list(object, "collectionOperations", location);
                let resources = self.ref_list(object, "resources", location);
                DeclKind::Resource(ResourceDecl {
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
        while let Some(key) = map.next_key::<String>()? {
            if !seen.insert(key.clone()) {
                return Err(ctx.fail(ParseAbort::DuplicateKey(key)));
            }
            match key.as_str() {
                "smithy" => {
                    let value = map.next_value_seed(NodeSeed { ctx, depth: 1 })?;
                    *state.version.borrow_mut() = Some(value);
                }
                "metadata" => match map.next_value_seed(NodeSeed { ctx, depth: 1 })? {
                    Node::Object(metadata) => state.document.borrow_mut().metadata = metadata,
                    other => state.error(
                        DiagnosticCode::InvalidProperty,
                        format!("`metadata` must be an object, found {}", other.type_name()),
                        state.at("/metadata".into()),
                    ),
                },
                "shapes" => map.next_value_seed(ShapesSeed { state })?,
                _ => {
                    map.next_value_seed(NodeSeed { ctx, depth: 1 })?;
                    state.error(
                        DiagnosticCode::UnknownProperty,
                        format!("unknown top-level property `{key}`"),
                        state.at(format!("/{}", escape_pointer_token(&key))),
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
        while let Some(key) = map.next_key::<String>()? {
            if seen.contains(&key) {
                return Err(ctx.fail(ParseAbort::DuplicateKey(key)));
            }
            if seen.len() >= state.options.max_shapes {
                return Err(ctx.fail(ParseAbort::ShapeLimit(state.options.max_shapes)));
            }
            let node = map.next_value_seed(NodeSeed { ctx, depth: 2 })?;
            if let Some(shape) = state.convert_shape(&key, node) {
                state
                    .document
                    .borrow_mut()
                    .shapes
                    .insert(shape.id.clone(), shape);
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
        ModelLoader::new().parse_str("test.json", json)
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
                r#""a#M":{"type":"structure","traits":{"smithy.api#mixin":{}}}"#,
                "/shapes/a#M/traits/smithy.api#mixin",
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
    fn prelude_namespace() {
        let json = doc(r#""smithy.api#String":{"type":"string"}"#);
        let err = parse(&json).unwrap_err();
        assert_eq!(codes(&err), [DiagnosticCode::PreludeConflict]);
        assert!(ModelLoader::new()
            .disable_prelude()
            .parse_str("p", &json)
            .is_ok());
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
        let loader = ModelLoader::new().max_shapes(1);
        assert!(loader.parse_str("t", &two).unwrap_err().is_resource_limit());
        assert!(ModelLoader::new()
            .max_shapes(2)
            .parse_str("t", &two)
            .is_ok());

        let loader = ModelLoader::new().max_input_bytes(10);
        assert!(loader.parse_str("t", &two).unwrap_err().is_resource_limit());
        assert!(loader
            .parse_reader("t", two.as_bytes())
            .unwrap_err()
            .is_resource_limit());
        let exact = ModelLoader::new().max_input_bytes(two.len() as u64);
        assert!(exact.parse_reader("t", two.as_bytes()).is_ok());
    }

    #[test]
    fn io_errors() {
        struct Failing;
        impl Read for Failing {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("boom"))
            }
        }
        let err = ModelLoader::new().parse_reader("r", Failing).unwrap_err();
        assert!(err.is_io());
        assert_eq!(codes(&err), [DiagnosticCode::Io]);
        let err = ModelLoader::new()
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
        assert_eq!(structure.member("a").unwrap().id, "a#S$a");
        let map = &document.shapes["a#M"];
        let names: Vec<_> = map.members().map(|m| m.id.as_str()).collect();
        assert_eq!(names, ["a#M$key", "a#M$value"]);
        let DeclKind::Operation(op) = &document.shapes["a#Op"].kind else {
            panic!()
        };
        assert_eq!(op.input.as_ref().unwrap(), "a#S");
        assert!(op.output.is_none());
        let DeclKind::Service(service) = &document.shapes["a#Svc"].kind else {
            panic!()
        };
        assert_eq!(service.rename["a#S"], "Renamed");
        let DeclKind::Resource(resource) = &document.shapes["a#R"].kind else {
            panic!()
        };
        assert_eq!(resource.identifiers["id"], "smithy.api#String");
        assert_eq!(resource.read.as_ref().unwrap(), "a#Op");
        assert_eq!(
            document.shapes["a#Blob"].traits["a#t"].pointer("/1/b"),
            Some(&Node::Null)
        );
        // Repeated targets share one allocation.
        let a = &document.shapes["a#S"].member("z").unwrap().target;
        let b = &document.shapes["a#M"].member("value").unwrap().target;
        assert!(Arc::ptr_eq(a.arc(), b.arc()));
    }
}
