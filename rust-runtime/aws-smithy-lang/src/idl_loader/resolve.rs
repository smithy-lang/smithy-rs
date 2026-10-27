/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Model-wide declaration index and Smithy IDL relative shape-ID resolution.
//!
//! A relative IDL shape ID resolves to a file-local `use` import, then to a shape of the same
//! name in the current namespace (declared by *any* input), then to a public prelude shape,
//! and otherwise to the current namespace. Step two requires every input's declarations, so
//! the assembler builds one [`DeclarationIndex`] before lowering any IDL document.

use super::syntax::{
    IdlDocument, NodeSyntax, PropertyValueSyntax, ShapeBodySyntax, ShapeIdSyntax, ShapeKindSyntax,
    StatementSyntax,
};
use crate::prelude;
use crate::shape::{Shape, ShapeType};
use crate::traits;
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

/// Shape names and kinds declared by every input of one assembly, plus resource identifiers
/// for target elision.
///
/// The index only resolves names; [`Merge`](crate::assembler) remains authoritative for
/// conflicting definitions. When provisional declarations disagree, lowering still picks one
/// deterministically (the first input for kinds; a typed resource before an IDL resource for
/// identifiers, which resolve in a second pass) while merge reports the conflict.
#[derive(Debug, Default)]
pub(crate) struct DeclarationIndex {
    kinds: HashMap<String, ShapeType>,
    identifiers: HashMap<String, IndexMap<String, String>>,
}

impl DeclarationIndex {
    /// Indexes a typed shape from a JSON document, an existing model, or code.
    pub(crate) fn add_shape(&mut self, shape: &Shape) {
        let id = shape.id().as_str();
        if self.kinds.contains_key(id) {
            return;
        }
        self.kinds.insert(id.to_owned(), shape.shape_type());
        if let Shape::Resource(resource) = shape {
            let identifiers = resource
                .identifiers
                .iter()
                .map(|(name, target)| (name.clone(), target.as_str().to_owned()))
                .collect();
            self.identifiers.insert(id.to_owned(), identifiers);
        }
    }

    /// Indexes the explicit and generated inline declarations of an IDL document.
    pub(crate) fn add_idl_names(&mut self, document: &IdlDocument) {
        let Some(namespace) = &document.namespace else {
            return;
        };
        let (input_suffix, output_suffix) = suffixes(document);
        for statement in &document.statements {
            let StatementSyntax::Shape(shape) = statement else {
                continue;
            };
            let id = format!("{}#{}", namespace.value, shape.name.value);
            if let ShapeBodySyntax::Properties(properties) = &shape.body {
                for property in properties {
                    if let PropertyValueSyntax::InlineStructure { .. } = property.value {
                        let suffix = match property.name.value.as_str() {
                            "input" => input_suffix,
                            "output" => output_suffix,
                            _ => continue,
                        };
                        self.kinds
                            .entry(format!("{id}{suffix}"))
                            .or_insert(ShapeType::Structure);
                    }
                }
            }
            self.kinds.entry(id).or_insert(kind(shape.kind));
        }
    }

    /// Records the identifiers of an IDL document's resources. Runs after every input's
    /// names are indexed, because identifier targets are themselves relative shape IDs.
    pub(crate) fn add_idl_resources(&mut self, document: &IdlDocument) {
        let Some(scope) = Scope::new(document, self) else {
            return;
        };
        let mut resources = Vec::new();
        for statement in &document.statements {
            let StatementSyntax::Shape(shape) = statement else {
                continue;
            };
            let (ShapeKindSyntax::Resource, ShapeBodySyntax::Properties(properties)) =
                (shape.kind, &shape.body)
            else {
                continue;
            };
            let id = scope.local(&shape.name.value);
            if self.identifiers.contains_key(&id) {
                continue;
            }
            let mut identifiers = IndexMap::new();
            for property in properties {
                if property.name.value != "identifiers" {
                    continue;
                }
                if let PropertyValueSyntax::Node(NodeSyntax::Object(entries, _)) = &property.value {
                    for (name, value) in entries {
                        if let Some(target) = scope.reference(value) {
                            identifiers.insert(name.value.clone(), target);
                        }
                    }
                }
            }
            resources.push((id, identifiers));
        }
        for (id, identifiers) in resources {
            self.identifiers.entry(id).or_insert(identifiers);
        }
    }

    /// The kind of a declared shape, falling back to the embedded prelude.
    pub(crate) fn kind(&self, id: &str) -> Option<ShapeType> {
        self.kinds
            .get(id)
            .copied()
            .or_else(|| prelude::shapes().get(id).map(Shape::shape_type))
    }

    /// The identifiers of a resource, if `id` is a known resource.
    pub(crate) fn identifiers(&self, id: &str) -> Option<&IndexMap<String, String>> {
        self.identifiers.get(id)
    }

    fn declares(&self, id: &str) -> bool {
        self.kinds.contains_key(id)
    }
}

/// The `operationInputSuffix` and `operationOutputSuffix` controls, or their defaults.
/// Invalid values are reported while lowering; the defaults are used for them here.
pub(crate) fn suffixes(document: &IdlDocument) -> (&str, &str) {
    let mut input = "Input";
    let mut output = "Output";
    for (key, value) in &document.controls {
        if let NodeSyntax::String(value) = value {
            match key.value.as_str() {
                "operationInputSuffix" => input = &value.value,
                "operationOutputSuffix" => output = &value.value,
                _ => {}
            }
        }
    }
    (input, output)
}

fn kind(kind: ShapeKindSyntax) -> ShapeType {
    match kind {
        ShapeKindSyntax::Blob => ShapeType::Blob,
        ShapeKindSyntax::Boolean => ShapeType::Boolean,
        ShapeKindSyntax::Document => ShapeType::Document,
        ShapeKindSyntax::String => ShapeType::String,
        ShapeKindSyntax::Byte => ShapeType::Byte,
        ShapeKindSyntax::Short => ShapeType::Short,
        ShapeKindSyntax::Integer => ShapeType::Integer,
        ShapeKindSyntax::Long => ShapeType::Long,
        ShapeKindSyntax::Float => ShapeType::Float,
        ShapeKindSyntax::Double => ShapeType::Double,
        ShapeKindSyntax::BigInteger => ShapeType::BigInteger,
        ShapeKindSyntax::BigDecimal => ShapeType::BigDecimal,
        ShapeKindSyntax::Timestamp => ShapeType::Timestamp,
        // `set` is rejected while lowering; index it as the list it would have been.
        ShapeKindSyntax::List | ShapeKindSyntax::Set => ShapeType::List,
        ShapeKindSyntax::Map => ShapeType::Map,
        ShapeKindSyntax::Structure => ShapeType::Structure,
        ShapeKindSyntax::Union => ShapeType::Union,
        ShapeKindSyntax::Enum => ShapeType::Enum,
        ShapeKindSyntax::IntEnum => ShapeType::IntEnum,
        ShapeKindSyntax::Service => ShapeType::Service,
        ShapeKindSyntax::Resource => ShapeType::Resource,
        ShapeKindSyntax::Operation => ShapeType::Operation,
    }
}

/// The file-local resolution context of one IDL document's shape section.
pub(crate) struct Scope<'a> {
    pub(crate) namespace: &'a str,
    imports: HashMap<&'a str, &'a str>,
    pub(crate) index: &'a DeclarationIndex,
}

impl<'a> Scope<'a> {
    /// The scope of a document's shape section, or `None` when it has no namespace (and so
    /// no shape statements).
    pub(crate) fn new(document: &'a IdlDocument, index: &'a DeclarationIndex) -> Option<Self> {
        let namespace = document.namespace.as_ref()?.value.as_str();
        let imports = document
            .uses
            .iter()
            .map(|id| (id.root_name(), id.value.as_str()))
            .collect();
        Some(Self {
            namespace,
            imports,
            index,
        })
    }

    /// The absolute ID of a shape declared in this document.
    pub(crate) fn local(&self, name: &str) -> String {
        format!("{}#{name}", self.namespace)
    }

    /// Resolves a parsed shape ID to its absolute form, including any member.
    pub(crate) fn resolve(&self, id: &ShapeIdSyntax) -> String {
        if id.absolute {
            id.value.clone()
        } else {
            self.resolve_relative(&id.value)
        }
    }

    /// Resolves the text of a shape ID that was written as a quoted string.
    pub(crate) fn resolve_text(&self, text: &str) -> String {
        if text.contains('#') {
            text.to_owned()
        } else {
            self.resolve_relative(text)
        }
    }

    fn resolve_relative(&self, value: &str) -> String {
        let (root, member) = match value.split_once('$') {
            Some((root, member)) => (root, Some(member)),
            None => (value, None),
        };
        let mut resolved = if let Some(import) = self.imports.get(root) {
            (*import).to_owned()
        } else {
            let local = self.local(root);
            if self.index.declares(&local) {
                local
            } else if is_public_prelude_name(root) {
                format!("{}#{root}", prelude::NAMESPACE)
            } else {
                local
            }
        };
        if let Some(member) = member {
            resolved.push('$');
            resolved.push_str(member);
        }
        resolved
    }

    /// The absolute target of a reference written as a syntactic shape ID or quoted string.
    pub(crate) fn reference(&self, node: &NodeSyntax) -> Option<String> {
        match node {
            NodeSyntax::SyntacticShapeId(id) => Some(self.resolve(id)),
            NodeSyntax::String(text) => Some(self.resolve_text(&text.value)),
            _ => None,
        }
    }
}

/// Metadata precedes `namespace` and `use`, so its relative syntactic shape IDs are qualified
/// directly into the prelude namespace.
pub(crate) fn resolve_metadata(id: &ShapeIdSyntax) -> String {
    if id.absolute {
        id.value.clone()
    } else {
        format!("{}#{}", prelude::NAMESPACE, id.value)
    }
}

/// Whether the prelude publicly defines `name`. Used even when the prelude is disabled for
/// the assembled model, so resolution never depends on assembly options.
fn is_public_prelude_name(name: &str) -> bool {
    static PUBLIC: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
        prelude::shapes()
            .values()
            .filter(|shape| !shape.traits().contains(&traits::PRIVATE))
            .map(|shape| shape.id().name())
            .collect()
    });
    PUBLIC.contains(name)
}
