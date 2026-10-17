/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Private, representation-neutral conversion of generic [`Node`] shape values into the
//! crate's owned semantic [`Shape`] types.
//!
//! The [Smithy JSON AST] shape layout is the crate's interchange form for *unresolved*
//! shapes: a shape is an object with `type`, `traits`, and kind-specific properties, members
//! are `{"target": ..., "traits": ...}`, and entity references are `{"target": ...}`
//! wrappers. [`JsonLoader`](crate::json_loader::JsonLoader) streams one such [`Node`] per
//! shape straight from its tokenizer; a future Smithy IDL loader lowers its syntax tree into
//! the same layout. Both then call [`AstConverter::convert_shape`], so unknown properties,
//! malformed shape bodies, duplicate references, trait nodes, declaration order, and
//! source-located semantic diagnostics stay identical across representations.
//!
//! The converter owns the input's label, its diagnostics, and a shape-ID interner. It holds
//! no tokenizer or syntax state, and nothing about any input representation leaks into the
//! [`Shape`]s it produces.
//!
//! [Smithy JSON AST]: https://smithy.io/2.0/spec/json-ast.html

use crate::diagnostic::{
    escape_pointer_token, push_pointer_token, Diagnostic, DiagnosticCode, DiagnosticSet, Position,
    SourceLocation,
};
use crate::node::{Node, NodeObject, NodeValue};
use crate::shape::owned::Common;
use crate::shape::{
    EnumShape, IntEnumShape, ListShape, MapShape, MemberShape, Members, OperationShape,
    ResourceShape, ServiceShape, Shape, ShapeType, SimpleShape, StructureShape, TraitMap,
    UnionShape,
};
use crate::shape_id::ShapeId;
use indexmap::IndexMap;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Converts JSON-AST-shaped [`Node`] values into owned [`Shape`]s, collecting diagnostics.
///
/// One converter covers one model input: its diagnostics and interner are shared by every
/// shape in that input.
pub(crate) struct AstConverter {
    source: Arc<str>,
    diagnostics: RefCell<DiagnosticSet>,
    interner: RefCell<HashSet<ShapeId>>,
}

impl AstConverter {
    /// A converter for `source`, collecting at most `max_diagnostics` diagnostics.
    pub(crate) fn new(source: Arc<str>, max_diagnostics: usize) -> Self {
        Self {
            source,
            diagnostics: RefCell::new(DiagnosticSet::with_limit(max_diagnostics)),
            interner: RefCell::default(),
        }
    }

    /// The input's label, used to build locations and to assign node locations.
    pub(crate) fn source(&self) -> &Arc<str> {
        &self.source
    }

    /// The diagnostics collected while converting this input.
    pub(crate) fn into_diagnostics(self) -> DiagnosticSet {
        self.diagnostics.into_inner()
    }

    pub(crate) fn at(&self, pointer: String, position: Option<Position>) -> SourceLocation {
        SourceLocation::with_pointer(&self.source, pointer, position)
    }

    pub(crate) fn error(
        &self,
        code: DiagnosticCode,
        message: impl Into<String>,
        location: SourceLocation,
    ) {
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

    /// Converts one shape declaration, keyed by its absolute shape ID.
    ///
    /// `node` is the shape's JSON-AST-shaped object. Positions already attached to `node`
    /// and its descendants are turned into the input's location table here, so a caller only
    /// has to record where each value came from.
    pub(crate) fn convert_shape(&self, key: &str, mut node: Node) -> Option<Shape> {
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
