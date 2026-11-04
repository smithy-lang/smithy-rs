/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Test and benchmark support: renders a model loaded from JSON AST as equivalent Smithy IDL.
//!
//! This is not a general IDL writer. It handles the constructs in the repository's JSON AST
//! models (no mixins or apply) and deliberately uses IDL sugar where it is unambiguous:
//! relative shape IDs, `///` documentation, omitted annotation-trait values, and `= value`
//! assignments for enum values and defaults. Tests check the output loads to an equivalent
//! model, so any mistake here fails loudly rather than skewing results.

#![allow(dead_code)]

use aws_smithy_lang::{AstWriter, Model, Node, NodeObject, ShapeId, ShapeType};
use std::collections::{HashMap, HashSet};
use std::fmt::Write;

/// One rendered input.
pub enum Part {
    Idl { source: String, text: String },
    Json { source: String, text: String },
}

impl Part {
    pub fn text(&self) -> &str {
        match self {
            Part::Idl { text, .. } | Part::Json { text, .. } => text,
        }
    }

    pub fn add_to(
        &self,
        assembler: aws_smithy_lang::ModelAssembler,
    ) -> aws_smithy_lang::ModelAssembler {
        match self {
            Part::Idl { source, text } => assembler.add_idl_str(source.clone(), text.clone()),
            Part::Json { source, text } => assembler.add_json_str(source.clone(), text.clone()),
        }
    }
}

/// How to split a model's shapes into inputs.
#[derive(Clone, Copy)]
pub enum Layout {
    /// One IDL file per namespace (plus one for metadata).
    PerNamespace,
    /// Shapes dealt round-robin into `n` IDL files per namespace, so most references cross
    /// files.
    Files(usize),
    /// Shapes dealt alternately into an IDL file and a JSON AST document per namespace.
    Mixed,
}

/// Renders `model` (without its prelude) as inputs that assemble to an equivalent model.
pub fn render(model: &Model, layout: Layout) -> Vec<Part> {
    // Read the insertion-ordered JSON AST back as a node by wrapping it in metadata.
    let ast = AstWriter::new().to_string(model).unwrap();
    let wrapped = format!(r#"{{"smithy": "2.0", "metadata": {{"ast": {ast}}}}}"#);
    let wrapper = Model::assembler()
        .disable_prelude()
        .max_depth(100)
        .add_json_str("ast.json", wrapped)
        .assemble()
        .into_result()
        .unwrap();
    let ast = wrapper.metadata_value("ast").unwrap().as_object().unwrap();
    let shapes = ast.get("shapes").and_then(Node::as_object);

    // Declared names per namespace decide when a relative ID is unambiguous.
    let mut declared: HashMap<String, HashSet<String>> = HashMap::new();
    let mut namespaces: Vec<String> = Vec::new();
    for (id, _) in shapes.into_iter().flat_map(NodeObject::iter) {
        let id = ShapeId::new(id).unwrap();
        if !declared.contains_key(id.namespace()) {
            namespaces.push(id.namespace().to_owned());
        }
        declared
            .entry(id.namespace().to_owned())
            .or_default()
            .insert(id.name().to_owned());
    }
    let structures: HashSet<String> = model
        .shapes()
        .filter(|s| s.shape_type() == ShapeType::Structure)
        .map(|s| s.id().to_string())
        .collect();
    let writer = Writer {
        declared: &declared,
        structures: &structures,
    };

    let mut parts = Vec::new();
    if let Some(metadata) = ast.get("metadata").and_then(Node::as_object) {
        let mut text = String::from("$version: \"2\"\n");
        for (key, value) in metadata.iter() {
            let _ = writeln!(text, "metadata {} = {}", quote(key), value.to_json_string());
        }
        parts.push(Part::Idl {
            source: "metadata.smithy".into(),
            text,
        });
    }
    for namespace in &namespaces {
        let in_namespace: Vec<_> = shapes
            .into_iter()
            .flat_map(NodeObject::iter)
            .filter(|(id, _)| ShapeId::new(id).unwrap().namespace() == namespace)
            .collect();
        let buckets = match layout {
            Layout::PerNamespace => 1,
            Layout::Files(n) => n.max(1),
            Layout::Mixed => 2,
        };
        for bucket in 0..buckets {
            let chosen = in_namespace
                .iter()
                .enumerate()
                .filter(|(i, _)| i % buckets == bucket)
                .map(|(_, shape)| *shape);
            if matches!(layout, Layout::Mixed) && bucket == 1 {
                let mut object = NodeObject::new();
                for (id, shape) in chosen {
                    object.insert(id, shape.clone());
                }
                let mut document = NodeObject::new();
                document.insert("smithy", "2.0");
                document.insert("shapes", Node::from(object));
                parts.push(Part::Json {
                    source: format!("{namespace}.json"),
                    text: Node::from(document).to_json_string(),
                });
                continue;
            }
            let mut text = format!("$version: \"2\"\nnamespace {namespace}\n");
            for (id, shape) in chosen {
                text.push('\n');
                writer.shape(&mut text, namespace, &ShapeId::new(id).unwrap(), shape);
            }
            parts.push(Part::Idl {
                source: format!("{namespace}-{bucket}.smithy"),
                text,
            });
        }
    }
    parts
}

/// Assembles rendered inputs.
pub fn assemble(parts: &[Part]) -> Result<Model, aws_smithy_lang::LoadError> {
    parts
        .iter()
        .fold(Model::assembler(), |assembler, part| part.add_to(assembler))
        .assemble()
        .into_result()
}

fn quote(value: &str) -> String {
    Node::from(value).to_json_string()
}

const DOCUMENTATION: &str = "smithy.api#documentation";
const DEFAULT: &str = "smithy.api#default";
const ENUM_VALUE: &str = "smithy.api#enumValue";

struct Writer<'a> {
    declared: &'a HashMap<String, HashSet<String>>,
    structures: &'a HashSet<String>,
}

impl Writer<'_> {
    /// The shortest ID that resolves back to `id` from `namespace`.
    fn reference(&self, namespace: &str, id: &str) -> String {
        let parsed = ShapeId::new(id).unwrap();
        let declares = |name: &str| {
            self.declared
                .get(namespace)
                .is_some_and(|n| n.contains(name))
        };
        let root = if parsed.namespace() == namespace
            || (parsed.namespace() == "smithy.api" && !declares(parsed.name()))
        {
            parsed.name().to_owned()
        } else {
            format!("{}#{}", parsed.namespace(), parsed.name())
        };
        match parsed.member() {
            Some(member) => format!("{root}${member}"),
            None => root,
        }
    }

    fn target(&self, namespace: &str, node: &Node) -> String {
        let id = node
            .as_object()
            .unwrap()
            .get("target")
            .unwrap()
            .as_str()
            .unwrap();
        self.reference(namespace, id)
    }

    /// Documentation comments and trait applications, each on its own line. Returns the
    /// value of `sugar`, which the caller writes as an `= value` assignment.
    fn traits(
        &self,
        out: &mut String,
        indent: &str,
        namespace: &str,
        traits: Option<&Node>,
        sugar: Option<&str>,
    ) -> Option<String> {
        let traits = traits.and_then(Node::as_object)?;
        if let Some(docs) = traits.get(DOCUMENTATION).and_then(Node::as_str) {
            if !docs.contains('\r') {
                for line in docs.split('\n') {
                    let _ = writeln!(out, "{indent}/// {line}");
                }
            }
        }
        let mut assigned = None;
        for (id, value) in traits.iter() {
            if id == DOCUMENTATION && value.as_str().is_some_and(|d| !d.contains('\r')) {
                continue;
            }
            if Some(id) == sugar {
                assigned = Some(value.to_json_string());
                continue;
            }
            let name = self.reference(namespace, id);
            let omit =
                value.as_object().is_some_and(NodeObject::is_empty) && self.structures.contains(id);
            if omit {
                let _ = writeln!(out, "{indent}@{name}");
            } else {
                let _ = writeln!(out, "{indent}@{name}({})", value.to_json_string());
            }
        }
        assigned
    }

    fn member(
        &self,
        out: &mut String,
        namespace: &str,
        name: &str,
        member: &Node,
        enum_member: bool,
    ) {
        let object = member.as_object().unwrap();
        let sugar = if enum_member { ENUM_VALUE } else { DEFAULT };
        let value = self.traits(out, "    ", namespace, object.get("traits"), Some(sugar));
        if enum_member {
            let _ = write!(out, "    {name}");
        } else {
            let _ = write!(out, "    {name}: {}", self.target(namespace, member));
        }
        match value {
            Some(value) => {
                let _ = writeln!(out, " = {value}");
            }
            None => out.push('\n'),
        }
    }

    fn shape(&self, out: &mut String, namespace: &str, id: &ShapeId, shape: &Node) {
        let object = shape.as_object().unwrap();
        let kind = object.get("type").unwrap().as_str().unwrap();
        self.traits(out, "", namespace, object.get("traits"), None);
        let name = id.name();
        match kind {
            "list" => {
                let _ = writeln!(out, "list {name} {{");
                self.member(
                    out,
                    namespace,
                    "member",
                    object.get("member").unwrap(),
                    false,
                );
                out.push_str("}\n");
            }
            "map" => {
                let _ = writeln!(out, "map {name} {{");
                self.member(out, namespace, "key", object.get("key").unwrap(), false);
                self.member(out, namespace, "value", object.get("value").unwrap(), false);
                out.push_str("}\n");
            }
            "structure" | "union" | "enum" | "intEnum" => {
                let _ = writeln!(out, "{kind} {name} {{");
                let enum_like = matches!(kind, "enum" | "intEnum");
                if let Some(members) = object.get("members").and_then(Node::as_object) {
                    for (member, value) in members.iter() {
                        self.member(out, namespace, member, value, enum_like);
                    }
                }
                out.push_str("}\n");
            }
            "service" | "resource" => {
                let _ = writeln!(out, "{kind} {name} {{");
                for (key, value) in object.iter() {
                    if matches!(key, "type" | "traits") {
                        continue;
                    }
                    let _ = writeln!(out, "    {key}: {}", self.property(namespace, key, value));
                }
                out.push_str("}\n");
            }
            "operation" => {
                let _ = writeln!(out, "operation {name} {{");
                for key in ["input", "output"] {
                    if let Some(value) = object.get(key) {
                        let _ = writeln!(out, "    {key}: {}", self.target(namespace, value));
                    }
                }
                if let Some(errors) = object.get("errors").and_then(Node::as_array) {
                    let errors: Vec<_> = errors.iter().map(|e| self.target(namespace, e)).collect();
                    let _ = writeln!(out, "    errors: [{}]", errors.join(", "));
                }
                out.push_str("}\n");
            }
            simple => {
                let _ = writeln!(out, "{simple} {name}");
            }
        }
    }

    /// An entity property: references become shape IDs; everything else stays a node.
    fn property(&self, namespace: &str, key: &str, value: &Node) -> String {
        let is_ref = |node: &Node| {
            node.as_object().is_some_and(|o| {
                o.len() == 1 && o.get("target").is_some_and(|t| t.as_str().is_some())
            })
        };
        if key == "rename" {
            return value.to_json_string();
        }
        if is_ref(value) {
            return self.target(namespace, value);
        }
        if let Some(items) = value.as_array() {
            if items.iter().all(is_ref) {
                let refs: Vec<_> = items.iter().map(|i| self.target(namespace, i)).collect();
                return format!("[{}]", refs.join(", "));
            }
        }
        if let Some(object) = value.as_object() {
            if !object.is_empty() && object.iter().all(|(_, v)| is_ref(v)) {
                let entries: Vec<_> = object
                    .iter()
                    .map(|(k, v)| format!("{}: {}", quote(k), self.target(namespace, v)))
                    .collect();
                return format!("{{ {} }}", entries.join(", "));
            }
        }
        value.to_json_string()
    }
}
