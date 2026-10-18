/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Lowers a parsed IDL document into the crate's typed [`Document`].
//!
//! Each shape is desugared into the generic JSON AST shape layout (a [`Node`] object with
//! `type`, `traits`, members, and `{"target": ...}` references) and handed to the same
//! [`AstConverter`] the JSON loader uses, so validation of shape bodies, references, and
//! traits is identical for both representations. Lowering only resolves shape IDs and
//! expands IDL sugar: documentation comments, omitted trait values, enum and default value
//! assignments, inline operation input/output, and resource-bound target elision.
//!
//! Positions follow the semantic location policy shared with the JSON loader: an object
//! value is located at its key, a string in an array at itself, an object in an array at its
//! first key, and every other value at the closest enclosing position.

use super::resolve::{resolve_metadata, suffixes, DeclarationIndex, Scope};
use super::syntax::{
    Annotations, ApplySyntax, IdlDocument, MemberNameSyntax, MemberSyntax, NodeSyntax,
    PropertySyntax, PropertyValueSyntax, ShapeBodySyntax, ShapeIdSyntax, ShapeKindSyntax,
    ShapeSyntax, Span, StatementSyntax, TraitValueSyntax,
};
use crate::assembler::merge_nodes;
use crate::ast_converter::AstConverter;
use crate::diagnostic::{
    escape_pointer_token, Diagnostic, DiagnosticCode, DiagnosticSet, LoadError, Position,
    SourceLocation,
};
use crate::json_loader::Document;
use crate::model::ModelData;
use crate::node::{Node, NodeObject, NodeValue};
use crate::prelude;
use crate::shape::ShapeType;
use crate::shape_id::ShapeId;
use crate::traits;
use indexmap::IndexMap;

const DEFAULT: &str = "smithy.api#default";

/// A successfully lowered IDL document and the side data assembly still needs.
pub(crate) struct LoweredDocument {
    pub(crate) document: Document,
    /// Non-error diagnostics, such as ignored control statements.
    pub(crate) diagnostics: DiagnosticSet,
    /// Syntactic shape IDs to check once every input is assembled.
    pub(crate) syntactic_ids: Vec<PendingSyntacticId>,
}

/// An unquoted node value that was resolved to an absolute shape ID string.
pub(crate) struct PendingSyntacticId {
    id: String,
    location: SourceLocation,
}

impl PendingSyntacticId {
    /// A [`SyntacticShapeIdTarget`](DiagnosticCode::SyntacticShapeIdTarget) warning if the ID
    /// does not resolve in the assembled model.
    pub(crate) fn check(self, data: &ModelData) -> Option<Diagnostic> {
        let resolves = ShapeId::new(&self.id).is_ok_and(|id| match data.root(&id) {
            None => false,
            Some(shape) => match id.member() {
                None => true,
                Some(name) => shape.members().any(|member| member.name() == name),
            },
        });
        (!resolves).then(|| {
            Diagnostic::warning(
                DiagnosticCode::SyntacticShapeIdTarget,
                format!(
                    "syntactic shape ID `{}` does not target a shape; quote the value if it is \
                     meant to be a string",
                    self.id
                ),
            )
            .with_source_location(self.location)
        })
    }
}

/// Lowers one parsed document. Every relative shape ID is resolved against `index`.
pub(crate) fn lower(
    document: &IdlDocument,
    index: &DeclarationIndex,
    max_diagnostics: usize,
) -> Result<LoweredDocument, LoadError> {
    let mut lowerer = Lowerer {
        document,
        index,
        converter: AstConverter::new(document.source.clone(), max_diagnostics),
        syntactic_ids: Vec::new(),
        output: Document::default(),
    };
    lowerer.controls()?;
    lowerer.metadata();
    if let Some(scope) = Scope::new(document, index) {
        lowerer.statements(&scope);
    }
    let Lowerer {
        converter,
        syntactic_ids,
        output,
        ..
    } = lowerer;
    let diagnostics = converter.into_diagnostics();
    if diagnostics.has_errors() {
        return Err(LoadError::invalid_model(diagnostics));
    }
    Ok(LoweredDocument {
        document: output,
        diagnostics,
        syntactic_ids,
    })
}

/// How a structure's elided (`$name`) members find their targets.
enum Binding<'a> {
    /// No `for` clause.
    None,
    /// The `for` clause was already reported as invalid.
    Invalid,
    /// Bound to a resource with these identifiers.
    Resource(String, Option<&'a IndexMap<String, String>>),
}

struct Lowerer<'a> {
    document: &'a IdlDocument,
    index: &'a DeclarationIndex,
    converter: AstConverter,
    syntactic_ids: Vec<PendingSyntacticId>,
    output: Document,
}

impl<'a> Lowerer<'a> {
    fn position(&self, span: Span) -> Option<Position> {
        Some(self.document.position(span))
    }

    /// A location without a semantic pointer, for syntax with no model counterpart.
    fn located(&self, span: Span) -> SourceLocation {
        SourceLocation::at_position(self.document.source.clone(), self.document.position(span))
    }

    fn at(&self, pointer: String, span: Span) -> SourceLocation {
        self.converter.at(pointer, self.position(span))
    }

    // --- Controls and metadata --------------------------------------------------------------

    fn controls(&mut self) -> Result<(), LoadError> {
        for (key, value) in &self.document.controls {
            let location = self.located(key.span);
            match key.value.as_str() {
                "version" => match value {
                    NodeSyntax::String(version)
                        if matches!(version.value.as_str(), "2" | "2.0") => {}
                    NodeSyntax::String(version) if is_version(&version.value) => {
                        // Other diagnostics may be artifacts of a different grammar.
                        let mut only = DiagnosticSet::default();
                        only.push(Diagnostic::error_at(
                            DiagnosticCode::UnsupportedVersion,
                            format!(
                                "unsupported Smithy version `{}`; only `2` and `2.0` are supported",
                                version.value
                            ),
                            Some(location),
                        ));
                        return Err(LoadError::invalid_model(only));
                    }
                    NodeSyntax::String(version) => self.converter.error(
                        DiagnosticCode::InvalidProperty,
                        format!(
                            "`$version` value `{}` is not a version number",
                            version.value
                        ),
                        location,
                    ),
                    other => self.converter.error(
                        DiagnosticCode::InvalidProperty,
                        format!("`$version` must be a string, found {}", type_name(other)),
                        location,
                    ),
                },
                "operationInputSuffix" | "operationOutputSuffix" => {
                    if !matches!(value, NodeSyntax::String(_)) {
                        self.converter.error(
                            DiagnosticCode::InvalidProperty,
                            format!(
                                "`${}` must be a string, found {}",
                                key.value,
                                type_name(value)
                            ),
                            location,
                        );
                    }
                }
                other => self.converter.push(
                    Diagnostic::warning(
                        DiagnosticCode::UnknownProperty,
                        format!("ignoring unknown control statement `${other}`"),
                    )
                    .with_source_location(location),
                ),
            }
        }
        Ok(())
    }

    fn metadata(&mut self) {
        let Some(first) = self.document.metadata.first() else {
            return;
        };
        let first = self.position(first.key.span);
        let mut object = NodeObject::new();
        for statement in &self.document.metadata {
            let value = self
                .node(None, &statement.value)
                .with_position(self.position(statement.key.span));
            object.insert(statement.key.value.clone(), value);
        }
        let mut node = Node::from(object);
        node.assign_locations(
            self.converter.source(),
            &mut String::from("/metadata"),
            first,
        );
        if let NodeValue::Object(metadata) = node.into_value() {
            self.output.metadata = metadata;
        }
    }

    // --- Nodes and traits -------------------------------------------------------------------

    /// Lowers a node value. `scope` is `None` in the metadata section.
    fn node(&mut self, scope: Option<&Scope<'_>>, node: &NodeSyntax) -> Node {
        match node {
            NodeSyntax::Null(_) => Node::null(),
            NodeSyntax::Bool(value, _) => Node::from(*value),
            NodeSyntax::Number(value) => Node::from(value.value),
            NodeSyntax::String(value) => {
                Node::from(value.value.clone()).with_position(self.position(value.span))
            }
            NodeSyntax::SyntacticShapeId(id) => {
                let resolved = match scope {
                    Some(scope) => scope.resolve(id),
                    None => resolve_metadata(id),
                };
                self.syntactic_ids.push(PendingSyntacticId {
                    id: resolved.clone(),
                    location: self.located(id.span),
                });
                Node::from(resolved).with_position(self.position(id.span))
            }
            NodeSyntax::Array(values, _) => Node::from(
                values
                    .iter()
                    .map(|value| self.node(scope, value))
                    .collect::<Vec<_>>(),
            ),
            NodeSyntax::Object(entries, _) => {
                let first = entries.first().and_then(|(key, _)| self.position(key.span));
                let mut object = NodeObject::new();
                for (key, value) in entries {
                    let value = self
                        .node(scope, value)
                        .with_position(self.position(key.span));
                    object.insert(key.value.clone(), value);
                }
                Node::from(object).with_position(first)
            }
        }
    }

    /// The value of a trait applied without one: `{}` for structure and map traits (and for
    /// traits without a known definition, following Smithy Java), `[]` for list traits, and
    /// `null` otherwise.
    fn omitted(&self, id: &str) -> Node {
        match self.index.kind(id) {
            None | Some(ShapeType::Structure | ShapeType::Map) => Node::from(NodeObject::new()),
            Some(ShapeType::List) => Node::from(Vec::new()),
            Some(_) => Node::null(),
        }
    }

    /// The `traits` object of a shape or member, or `None` when it has none.
    ///
    /// `sugar` holds traits synthesized from value assignments and inline I/O, applied after
    /// the explicit traits.
    fn traits(
        &mut self,
        scope: &Scope<'_>,
        annotations: &Annotations,
        sugar: Vec<(String, Node)>,
        pointer: &str,
    ) -> Option<Node> {
        let mut first = None;
        let mut object = NodeObject::new();
        if let Some(documentation) = &annotations.documentation {
            let position = self.position(documentation.span);
            first = first.or(position);
            object.insert(
                traits::DOCUMENTATION.as_str(),
                Node::from(documentation.value.clone()).with_position(position),
            );
        }
        for applied in &annotations.traits {
            let id = scope.resolve(&applied.id);
            let value = match &applied.value {
                TraitValueSyntax::Omitted { at } => {
                    self.omitted(&id).with_position(self.position(*at))
                }
                TraitValueSyntax::Explicit(value) => self
                    .node(Some(scope), value)
                    .with_position(self.position(applied.span)),
            };
            first = first.or(value.position());
            self.apply_trait(&mut object, id, value, pointer);
        }
        for (id, value) in sugar {
            first = first.or(value.position());
            self.apply_trait(&mut object, id, value, pointer);
        }
        (!object.is_empty()).then(|| Node::from(object).with_position(first))
    }

    /// Adds a trait, merging a repeated application with the assembler's trait rules.
    fn apply_trait(&mut self, traits: &mut NodeObject, id: String, value: Node, pointer: &str) {
        let Some(previous) = traits.get(&id) else {
            traits.insert(id, value);
            return;
        };
        let location = self.converter.at(
            format!("{pointer}/traits/{}", escape_pointer_token(&id)),
            value.position(),
        );
        let merged = merge_nodes(previous, value, |_, _| {
            Diagnostic::error(
                DiagnosticCode::TraitConflict,
                format!("conflicting `{id}` trait values"),
            )
            .with_source_location(location)
        });
        match merged {
            Ok(Some(merged)) => {
                traits.insert(id, merged);
            }
            Ok(None) => {}
            Err(diagnostic) => self.converter.push(diagnostic),
        }
    }

    /// `[{"target": ...}, ...]` for a `with [...]` clause, which the converter rejects with
    /// `UnsupportedFeature`.
    fn mixins(&mut self, scope: &Scope<'_>, mixins: &[ShapeIdSyntax]) -> Option<Node> {
        let first = self.position(mixins.first()?.span);
        let values = mixins
            .iter()
            .map(|id| {
                let position = self.position(id.span);
                let target = Node::from(scope.resolve(id)).with_position(position);
                Node::from(NodeObject::from_iter([("target", target)])).with_position(position)
            })
            .collect::<Vec<_>>();
        Some(Node::from(values).with_position(first))
    }

    // --- Shapes -----------------------------------------------------------------------------

    fn statements(&mut self, scope: &Scope<'_>) {
        for statement in &self.document.statements {
            match statement {
                StatementSyntax::Apply(apply) => self.apply(scope, apply),
                StatementSyntax::Shape(shape) => self.shape(scope, shape),
            }
        }
    }

    fn apply(&mut self, scope: &Scope<'_>, apply: &ApplySyntax) {
        self.converter.error(
            DiagnosticCode::UnsupportedFeature,
            format!(
                "`apply` to `{}` is not supported",
                scope.resolve(&apply.target)
            ),
            self.located(apply.span),
        );
    }

    fn shape(&mut self, scope: &Scope<'_>, shape: &ShapeSyntax) {
        let id = scope.local(&shape.name.value);
        let pointer = shape_pointer(&id);
        let mut object = NodeObject::new();
        object.insert(
            "type",
            Node::from(shape.kind.as_str()).with_position(self.position(shape.span)),
        );
        if let Some(traits) = self.traits(scope, &shape.annotations, Vec::new(), &pointer) {
            object.insert("traits", traits);
        }
        if let Some(mixins) = self.mixins(scope, &shape.mixins) {
            object.insert("mixins", mixins);
        }
        match &shape.body {
            ShapeBodySyntax::None => {}
            ShapeBodySyntax::Members(members) => {
                let binding = self.binding(scope, shape.resource.as_ref());
                let entries = self.members(
                    scope,
                    shape.kind,
                    members,
                    &binding,
                    !shape.mixins.is_empty(),
                    &pointer,
                );
                if matches!(
                    shape.kind,
                    ShapeKindSyntax::List | ShapeKindSyntax::Set | ShapeKindSyntax::Map
                ) {
                    for (name, member) in entries {
                        object.insert(name, member);
                    }
                } else {
                    let first = entries.first().and_then(|(_, member)| member.position());
                    let members: NodeObject = entries.into_iter().collect();
                    object.insert("members", Node::from(members).with_position(first));
                }
            }
            ShapeBodySyntax::Properties(properties) => {
                if shape.kind == ShapeKindSyntax::Operation {
                    self.inline_shapes(scope, shape, properties);
                }
                for property in properties {
                    let value = match &property.value {
                        PropertyValueSyntax::Node(value) => {
                            self.property(scope, shape.kind, &property.name.value, value)
                        }
                        PropertyValueSyntax::InlineStructure { span, .. } => {
                            let Some(suffix) = self.inline_suffix(property) else {
                                continue;
                            };
                            let generated = format!("{id}{suffix}");
                            let position = self.position(*span);
                            let target = Node::from(generated).with_position(position);
                            Node::from(NodeObject::from_iter([("target", target)]))
                        }
                    };
                    object.insert(
                        property.name.value.clone(),
                        value.with_position(self.position(property.name.span)),
                    );
                }
            }
        }
        let node = Node::from(object).with_position(self.position(shape.name.span));
        self.convert(id, node, shape.name.span);
    }

    /// Converts a lowered shape node and adds it to the document.
    fn convert(&mut self, id: String, node: Node, span: Span) {
        if self.output.shapes.contains_key(id.as_str()) {
            // Explicit duplicates are syntax errors; this is a generated inline shape
            // colliding with a declaration in the same file.
            let location = self.at(shape_pointer(&id), span);
            self.converter.error(
                DiagnosticCode::ShapeConflict,
                format!("`{id}` is defined more than once in this file"),
                location,
            );
            return;
        }
        if let Some(shape) = self.converter.convert_shape(&id, node) {
            self.output.shapes.insert(shape.id().clone(), shape);
        }
    }

    /// Resolves a `for` clause.
    fn binding(&mut self, scope: &Scope<'_>, resource: Option<&ShapeIdSyntax>) -> Binding<'a> {
        let Some(resource) = resource else {
            return Binding::None;
        };
        let id = scope.resolve(resource);
        let index = self.index;
        if index.kind(&id) == Some(ShapeType::Resource) {
            let identifiers = index.identifiers(&id);
            Binding::Resource(id, identifiers)
        } else {
            self.converter.error(
                DiagnosticCode::InvalidTarget,
                format!("`for` must reference a resource, but `{id}` is not a resource"),
                self.located(resource.span),
            );
            Binding::Invalid
        }
    }

    /// Lowers members to `(name, {"target": ..., "traits": ...})` entries.
    fn members(
        &mut self,
        scope: &Scope<'_>,
        kind: ShapeKindSyntax,
        members: &[MemberSyntax],
        binding: &Binding<'_>,
        has_mixins: bool,
        pointer: &str,
    ) -> Vec<(String, Node)> {
        let is_enum = matches!(kind, ShapeKindSyntax::Enum | ShapeKindSyntax::IntEnum);
        let top_level = matches!(
            kind,
            ShapeKindSyntax::List | ShapeKindSyntax::Set | ShapeKindSyntax::Map
        );
        let mut entries = Vec::with_capacity(members.len());
        for member in members {
            let name = member.name.value();
            let name_span = member.name.span();
            let member_pointer = if top_level {
                format!("{pointer}/{}", escape_pointer_token(name))
            } else {
                format!("{pointer}/members/{}", escape_pointer_token(name))
            };
            let target = match (&member.target, &member.name) {
                (Some(target), _) => Some(
                    Node::from(scope.resolve(target)).with_position(self.position(target.span)),
                ),
                (None, _) if is_enum => {
                    Some(Node::from(prelude::UNIT.as_str()).with_position(self.position(name_span)))
                }
                (None, MemberNameSyntax::Elided(_)) => {
                    self.elided(binding, name, has_mixins, member_pointer.clone(), name_span)
                }
                (None, MemberNameSyntax::Named(_)) => {
                    // The parser requires a target for every other named member.
                    self.converter.error(
                        DiagnosticCode::MissingProperty,
                        format!("member `{name}` is missing a target"),
                        self.at(member_pointer.clone(), name_span),
                    );
                    None
                }
            };
            let mut sugar = Vec::new();
            if let Some(value) = &member.value {
                let trait_id = if is_enum {
                    traits::ENUM_VALUE.as_str().to_owned()
                } else {
                    DEFAULT.to_owned()
                };
                let value = self
                    .node(Some(scope), value)
                    .with_position(self.position(value.span()));
                sugar.push((trait_id, value));
            }
            let traits = self.traits(scope, &member.annotations, sugar, &member_pointer);
            let Some(target) = target else {
                continue;
            };
            let mut object = NodeObject::new();
            object.insert("target", target);
            if let Some(traits) = traits {
                object.insert("traits", traits);
            }
            entries.push((
                name.to_owned(),
                Node::from(object).with_position(self.position(name_span)),
            ));
        }
        entries
    }

    /// The target of a `$name` member from its structure's resource binding.
    fn elided(
        &mut self,
        binding: &Binding<'_>,
        name: &str,
        has_mixins: bool,
        pointer: String,
        span: Span,
    ) -> Option<Node> {
        let message = match binding {
            Binding::Resource(_, Some(identifiers)) if identifiers.contains_key(name) => {
                let target = identifiers[name].clone();
                return Some(Node::from(target).with_position(self.position(span)));
            }
            // Already reported: an invalid `for` clause, or mixins, which are unsupported.
            Binding::Invalid => return None,
            _ if has_mixins => return None,
            Binding::Resource(resource, _) => format!(
                "cannot elide the target of `${name}`: resource `{resource}` has no identifier \
                 named `{name}`"
            ),
            Binding::None => {
                format!("cannot elide the target of `${name}` without a `for` resource binding")
            }
        };
        self.converter.error(
            DiagnosticCode::InvalidProperty,
            message,
            self.at(pointer, span),
        );
        None
    }

    // --- Entity properties ------------------------------------------------------------------

    /// Lowers a service, resource, or operation property to its JSON AST form.
    fn property(
        &mut self,
        scope: &Scope<'_>,
        kind: ShapeKindSyntax,
        name: &str,
        value: &NodeSyntax,
    ) -> Node {
        use ShapeKindSyntax::{Operation, Resource, Service};
        match (kind, name) {
            (Service, "operations" | "resources" | "errors")
            | (Resource, "operations" | "collectionOperations" | "resources")
            | (Operation, "errors") => match value {
                NodeSyntax::Array(values, _) => Node::from(
                    values
                        .iter()
                        .map(|value| self.reference(scope, value))
                        .collect::<Vec<_>>(),
                ),
                other => self.node(Some(scope), other),
            },
            (Resource, "create" | "put" | "read" | "update" | "delete" | "list")
            | (Operation, "input" | "output") => self.reference(scope, value),
            (Resource, "identifiers" | "properties") => match value {
                NodeSyntax::Object(entries, _) => {
                    let first = entries.first().and_then(|(key, _)| self.position(key.span));
                    let mut object = NodeObject::new();
                    for (key, value) in entries {
                        let value = self
                            .reference(scope, value)
                            .with_position(self.position(key.span));
                        object.insert(key.value.clone(), value);
                    }
                    Node::from(object).with_position(first)
                }
                other => self.node(Some(scope), other),
            },
            // Plain values (`version`, `rename`) and unknown properties, which the converter
            // reports.
            _ => self.node(Some(scope), value),
        }
    }

    /// `{"target": ...}` for a reference. A value that is not a shape ID is kept so the
    /// converter reports it.
    fn reference(&mut self, scope: &Scope<'_>, value: &NodeSyntax) -> Node {
        let target = match scope.reference(value) {
            Some(target) => Node::from(target).with_position(self.position(value.span())),
            None => self.node(Some(scope), value),
        };
        let position = target.position();
        Node::from(NodeObject::from_iter([("target", target)])).with_position(position)
    }

    /// The generated-name suffix of an inline `input` or `output` property.
    fn inline_suffix(&self, property: &PropertySyntax) -> Option<&'a str> {
        let (input, output) = suffixes(self.document);
        match property.name.value.as_str() {
            "input" => Some(input),
            "output" => Some(output),
            _ => None,
        }
    }

    /// Converts an operation's inline input, then output, structures, matching Smithy Java's
    /// load order (both precede the operation itself).
    fn inline_shapes(
        &mut self,
        scope: &Scope<'_>,
        shape: &ShapeSyntax,
        properties: &[PropertySyntax],
    ) {
        for name in ["input", "output"] {
            for property in properties {
                if property.name.value == name {
                    self.inline_shape(scope, shape, property);
                }
            }
        }
        for property in properties {
            if matches!(property.value, PropertyValueSyntax::InlineStructure { .. })
                && self.inline_suffix(property).is_none()
            {
                self.converter.error(
                    DiagnosticCode::InvalidProperty,
                    format!(
                        "only `input` and `output` can be defined inline, not `{}`",
                        property.name.value
                    ),
                    self.at(
                        format!(
                            "{}/{}",
                            shape_pointer(&scope.local(&shape.name.value)),
                            escape_pointer_token(&property.name.value)
                        ),
                        property.name.span,
                    ),
                );
            }
        }
    }

    fn inline_shape(
        &mut self,
        scope: &Scope<'_>,
        operation: &ShapeSyntax,
        property: &PropertySyntax,
    ) {
        let PropertyValueSyntax::InlineStructure {
            annotations,
            members,
            resource,
            mixins,
            span,
        } = &property.value
        else {
            return;
        };
        let Some(suffix) = self.inline_suffix(property) else {
            return;
        };
        let id = format!("{}{suffix}", scope.local(&operation.name.value));
        let pointer = shape_pointer(&id);
        let position = self.position(*span);
        let marker = if property.name.value == "input" {
            traits::INPUT.as_str().to_owned()
        } else {
            traits::OUTPUT.as_str().to_owned()
        };
        let mut object = NodeObject::new();
        object.insert("type", Node::from("structure").with_position(position));
        let sugar = vec![(
            marker,
            Node::from(NodeObject::new()).with_position(position),
        )];
        if let Some(traits) = self.traits(scope, annotations, sugar, &pointer) {
            object.insert("traits", traits);
        }
        if let Some(mixins) = self.mixins(scope, mixins) {
            object.insert("mixins", mixins);
        }
        let binding = self.binding(scope, resource.as_ref());
        let entries = self.members(
            scope,
            ShapeKindSyntax::Structure,
            members,
            &binding,
            !mixins.is_empty(),
            &pointer,
        );
        let first = entries.first().and_then(|(_, member)| member.position());
        let members: NodeObject = entries.into_iter().collect();
        object.insert("members", Node::from(members).with_position(first));
        self.convert(id, Node::from(object).with_position(position), *span);
    }
}

fn shape_pointer(id: &str) -> String {
    format!("/shapes/{}", escape_pointer_token(id))
}

/// `1*DIGIT ["." 1*DIGIT]`
fn is_version(value: &str) -> bool {
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    match value.split_once('.') {
        Some((major, minor)) => digits(major) && digits(minor),
        None => digits(value),
    }
}

fn type_name(node: &NodeSyntax) -> &'static str {
    match node {
        NodeSyntax::Null(_) => "null",
        NodeSyntax::Bool(..) => "boolean",
        NodeSyntax::Number(_) => "number",
        NodeSyntax::String(_) | NodeSyntax::SyntacticShapeId(_) => "string",
        NodeSyntax::Array(..) => "array",
        NodeSyntax::Object(..) => "object",
    }
}
