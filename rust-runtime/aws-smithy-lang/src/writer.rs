/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Deterministic Smithy 2.0 JSON AST serialization and semantic model equivalence.
//!
//! The writer serializes the declaration layer, never resolved state, so future `apply`
//! and mixin support cannot leak flattened members into output. Output bytes depend only on
//! a model's semantic content:
//!
//! - shape IDs, trait IDs, metadata keys, service `rename` keys, resource `identifiers` and
//!   `properties` keys, and every nested trait/metadata object key are sorted;
//! - unordered reference sets (service/resource `operations`, `resources`, and `errors`;
//!   resource `collectionOperations`; operation `errors`) are sorted;
//! - member order and arbitrary trait/metadata array order are preserved;
//! - operation `input`/`output` are always explicit, normalized to `smithy.api#Unit`;
//! - optional empty properties are omitted, as are source locations.
//!
//! Serialization borrows the model and does not clone trait values.
//!
//! Note: This is currently using `serde` to write out the model. In the future we might
//! base this off of the SDKs existing schema serde implementation, but we need to figure
//! out what we want the dependency graph to look like before we make that switch.

use crate::ast::{DeclKind, MemberDecl, ShapeDecl, TraitMap};
use crate::diagnostic::WriteError;
use crate::model::{unit_id, Model};
use crate::node::Node;
use crate::shape_id::ShapeId;
use crate::traits;
use indexmap::IndexMap;
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};
use std::borrow::Cow;
use std::io::Write;

/// Writes a [`Model`] as deterministic Smithy 2.0 JSON AST.
///
/// ```
/// use aws_smithy_lang::{Model, ModelWriter};
///
/// let model = Model::from_json_str("m", r#"{"smithy": "2", "shapes": {
///     "ex#B": {"type": "string"}, "ex#A": {"type": "operation"}
/// }}"#)?;
/// let json = ModelWriter::new().to_string(&model)?;
/// assert_eq!(
///     json,
///     r#"{"smithy":"2.0","shapes":{"ex#A":{"type":"operation","input":{"target":"smithy.api#Unit"},"output":{"target":"smithy.api#Unit"}},"ex#B":{"type":"string"}}}"#
/// );
/// # Ok::<_, Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, Default)]
pub struct ModelWriter {
    pretty: bool,
    include_prelude: bool,
}

impl ModelWriter {
    /// A writer producing compact output without prelude shapes.
    pub fn new() -> Self {
        Self::default()
    }

    /// Produces indented output (two spaces, trailing newline) instead of compact output.
    pub fn pretty(mut self, pretty: bool) -> Self {
        self.pretty = pretty;
        self
    }

    /// Also writes prelude shapes. Output written this way can only be reloaded with
    /// [`ModelLoader::disable_prelude`](crate::ModelLoader::disable_prelude).
    pub fn include_prelude(mut self, include: bool) -> Self {
        self.include_prelude = include;
        self
    }

    /// Serializes `model` to a string.
    pub fn to_string(&self, model: &Model) -> Result<String, WriteError> {
        let mut out = Vec::new();
        self.write(model, &mut out)?;
        Ok(String::from_utf8(out).expect("serde_json always produces UTF-8"))
    }

    /// Serializes `model` to `writer`. Wrap unbuffered writers in a [`std::io::BufWriter`].
    pub fn write(&self, model: &Model, mut writer: impl Write) -> Result<(), WriteError> {
        let document = DocumentSer {
            model,
            include_prelude: self.include_prelude,
            mode: Mode::Write,
        };
        if self.pretty {
            serde_json::to_writer_pretty(&mut writer, &document).map_err(WriteError::from_json)?;
            writer
                .write_all(b"\n")
                .map_err(|e| WriteError::from_json(serde_json::Error::io(e)))
        } else {
            serde_json::to_writer(writer, &document).map_err(WriteError::from_json)
        }
    }
}

impl Model {
    /// Returns `true` if both models have the same effective semantic content.
    ///
    /// Source locations, version spelling, object key order, and the order of unordered
    /// reference sets are ignored. Member order and trait/metadata array order are
    /// significant. An omitted enum value equals an explicit value equal to the member name,
    /// and an omitted operation input/output equals an explicit `smithy.api#Unit`. Models
    /// loaded with and without the prelude are never equivalent.
    pub fn equivalent(&self, other: &Model) -> bool {
        self.has_prelude() == other.has_prelude() && canonical_bytes(self) == canonical_bytes(other)
    }
}

fn canonical_bytes(model: &Model) -> Vec<u8> {
    let document = DocumentSer {
        model,
        include_prelude: false,
        mode: Mode::Effective,
    };
    serde_json::to_vec(&document).expect("serializing to memory cannot fail")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Declaration form, for output.
    Write,
    /// Effective form, for equivalence: synthesizes omitted enum values.
    Effective,
}

struct DocumentSer<'a> {
    model: &'a Model,
    include_prelude: bool,
    mode: Mode,
}

impl Serialize for DocumentSer<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let data = self.model.data();
        let mut shapes: Vec<&ShapeDecl> = data.document.shapes.values().collect();
        if self.include_prelude {
            shapes.extend(data.prelude.into_iter().flat_map(|p| p.shapes.values()));
        }
        shapes.sort_unstable_by(|a, b| a.id.cmp(&b.id));

        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("smithy", "2.0")?;
        let metadata = &data.document.metadata;
        if !metadata.is_empty() {
            map.serialize_entry("metadata", &SortedObject(metadata.iter().collect()))?;
        }
        if !shapes.is_empty() {
            let shapes = Entries(
                shapes
                    .into_iter()
                    .map(|decl| {
                        let shape = ShapeSer {
                            decl,
                            mode: self.mode,
                        };
                        (decl.id.as_str(), shape)
                    })
                    .collect(),
            );
            map.serialize_entry("shapes", &shapes)?;
        }
        map.end()
    }
}

/// Serializes ordered `(key, value)` pairs as an object.
struct Entries<'a, V>(Vec<(&'a str, V)>);

impl<V: Serialize> Serialize for Entries<'_, V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in &self.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

/// A node with every nested object's keys sorted.
struct Sorted<'a>(&'a Node);

impl Serialize for Sorted<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Node::Array(elements) => {
                let mut seq = serializer.serialize_seq(Some(elements.len()))?;
                for element in elements {
                    seq.serialize_element(&Sorted(element))?;
                }
                seq.end()
            }
            Node::Object(object) => SortedObject(object.iter().collect()).serialize(serializer),
            scalar => scalar.serialize(serializer),
        }
    }
}

/// Object entries, serialized sorted by key with sorted nested values.
struct SortedObject<'a>(Vec<(&'a str, &'a Node)>);

impl Serialize for SortedObject<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut entries = self.0.clone();
        entries.sort_unstable_by_key(|(key, _)| *key);
        let mut map = serializer.serialize_map(Some(entries.len()))?;
        for (key, value) in entries {
            map.serialize_entry(key, &Sorted(value))?;
        }
        map.end()
    }
}

struct TraitsSer<'a>(Vec<(&'a str, Cow<'a, Node>)>);

impl<'a> TraitsSer<'a> {
    fn new(traits: &'a TraitMap) -> Self {
        let mut entries: Vec<_> = traits
            .iter()
            .map(|(id, value)| (id.as_str(), Cow::Borrowed(value)))
            .collect();
        entries.sort_unstable_by_key(|(key, _)| *key);
        Self(entries)
    }
}

impl Serialize for TraitsSer<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in &self.0 {
            map.serialize_entry(key, &Sorted(value))?;
        }
        map.end()
    }
}

struct Ref<'a>(&'a ShapeId);

impl Serialize for Ref<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("target", self.0.as_str())?;
        map.end()
    }
}

struct RefSet<'a>(Vec<&'a ShapeId>);

impl<'a> RefSet<'a> {
    fn new(ids: &'a [ShapeId]) -> Self {
        let mut ids: Vec<_> = ids.iter().collect();
        ids.sort_unstable();
        Self(ids)
    }
}

impl Serialize for RefSet<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for id in &self.0 {
            seq.serialize_element(&Ref(id))?;
        }
        seq.end()
    }
}

fn sorted_ref_map(map: &IndexMap<String, ShapeId>) -> Entries<'_, Ref<'_>> {
    let mut entries: Vec<_> = map.iter().map(|(k, v)| (k.as_str(), Ref(v))).collect();
    entries.sort_unstable_by_key(|(key, _)| *key);
    Entries(entries)
}

struct MemberSer<'a> {
    decl: &'a MemberDecl,
    /// Synthesized `@enumValue` for an enum member that omits it (effective mode only).
    synthesized_enum_value: bool,
}

impl Serialize for MemberSer<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut traits = TraitsSer::new(&self.decl.traits);
        if self.synthesized_enum_value {
            traits
                .0
                .push((traits::ENUM_VALUE, Cow::Owned(Node::from(self.decl.name()))));
            traits.0.sort_unstable_by_key(|(key, _)| *key);
        }
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("target", self.decl.target.as_str())?;
        if !traits.0.is_empty() {
            map.serialize_entry("traits", &traits)?;
        }
        map.end()
    }
}

struct ShapeSer<'a> {
    decl: &'a ShapeDecl,
    mode: Mode,
}

impl<'a> ShapeSer<'a> {
    fn member(&self, decl: &'a MemberDecl) -> MemberSer<'a> {
        let synthesized_enum_value = self.mode == Mode::Effective
            && matches!(self.decl.kind, DeclKind::Enum(_))
            && !decl.traits.contains_key(traits::ENUM_VALUE);
        MemberSer {
            decl,
            synthesized_enum_value,
        }
    }
}

impl Serialize for ShapeSer<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("type", self.decl.shape_type().as_str())?;
        match &self.decl.kind {
            DeclKind::Simple(_) => {}
            DeclKind::List { member } => map.serialize_entry("member", &self.member(member))?,
            DeclKind::Map { key, value } => {
                map.serialize_entry("key", &self.member(key))?;
                map.serialize_entry("value", &self.member(value))?;
            }
            DeclKind::Structure(members)
            | DeclKind::Union(members)
            | DeclKind::Enum(members)
            | DeclKind::IntEnum(members) => {
                if !members.is_empty() {
                    let members = Entries(
                        members
                            .iter()
                            .map(|(name, decl)| (name.as_str(), self.member(decl)))
                            .collect(),
                    );
                    map.serialize_entry("members", &members)?;
                }
            }
            DeclKind::Service(service) => {
                if let Some(version) = &service.version {
                    map.serialize_entry("version", version)?;
                }
                for (key, ids) in [
                    ("operations", &service.operations),
                    ("resources", &service.resources),
                    ("errors", &service.errors),
                ] {
                    if !ids.is_empty() {
                        map.serialize_entry(key, &RefSet::new(ids))?;
                    }
                }
                if !service.rename.is_empty() {
                    let mut rename: Vec<_> = service
                        .rename
                        .iter()
                        .map(|(id, name)| (id.as_str(), name.as_str()))
                        .collect();
                    rename.sort_unstable_by_key(|(key, _)| *key);
                    map.serialize_entry("rename", &Entries(rename))?;
                }
            }
            DeclKind::Operation(operation) => {
                let unit = unit_id();
                map.serialize_entry("input", &Ref(operation.input.as_ref().unwrap_or(unit)))?;
                map.serialize_entry("output", &Ref(operation.output.as_ref().unwrap_or(unit)))?;
                if !operation.errors.is_empty() {
                    map.serialize_entry("errors", &RefSet::new(&operation.errors))?;
                }
            }
            DeclKind::Resource(resource) => {
                if !resource.identifiers.is_empty() {
                    map.serialize_entry("identifiers", &sorted_ref_map(&resource.identifiers))?;
                }
                if !resource.properties.is_empty() {
                    map.serialize_entry("properties", &sorted_ref_map(&resource.properties))?;
                }
                for (key, id) in resource.lifecycle() {
                    if let Some(id) = id {
                        map.serialize_entry(key, &Ref(id))?;
                    }
                }
                for (key, ids) in [
                    ("operations", &resource.operations),
                    ("collectionOperations", &resource.collection_operations),
                    ("resources", &resource.resources),
                ] {
                    if !ids.is_empty() {
                        map.serialize_entry(key, &RefSet::new(ids))?;
                    }
                }
            }
        }
        if !self.decl.traits.is_empty() {
            map.serialize_entry("traits", &TraitsSer::new(&self.decl.traits))?;
        }
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::ModelLoader;
    use std::error::Error as _;

    fn load(json: &str) -> Model {
        Model::from_json_str("t", json).unwrap()
    }

    const A: &str = r#"{"smithy":"2","metadata":{"z":1,"a":{"y":[3,1],"b":null}},"shapes":{
        "ex#Svc":{"type":"service","version":"1","operations":[{"target":"ex#Op2"},{"target":"ex#Op1"}],
                  "rename":{"ex#Str":"S2","ex#E":"E2"}},
        "ex#Op1":{"type":"operation","errors":[{"target":"ex#Err2"},{"target":"ex#Err1"}]},
        "ex#Op2":{"type":"operation","input":{"target":"ex#S"},"output":{"target":"smithy.api#Unit"}},
        "ex#Err1":{"type":"structure","traits":{"smithy.api#error":"client"}},
        "ex#Err2":{"type":"structure","traits":{"smithy.api#error":"server"}},
        "ex#S":{"type":"structure","members":{"z":{"target":"ex#Str"},"a":{"target":"ex#E"}},
                "traits":{"smithy.api#input":{},"smithy.api#documentation":"doc"}},
        "ex#Str":{"type":"string","traits":{"ex#custom":{"k2":[{"b":1,"a":1.5}],"k1":true}}},
        "ex#E":{"type":"enum","members":{"B":{"target":"smithy.api#Unit"},"A":{"target":"smithy.api#Unit"}}},
        "ex#R":{"type":"resource","identifiers":{"z":{"target":"ex#Str"},"a":{"target":"ex#Str"}},
                "read":{"target":"ex#Op1"},"operations":[{"target":"ex#Op2"},{"target":"ex#Op1"}],"mixins":[]}
    }}"#;

    /// The same model with every unordered collection reordered and spellings changed.
    const B: &str = r#"{"shapes":{
        "ex#R":{"operations":[{"target":"ex#Op1"},{"target":"ex#Op2"}],"read":{"target":"ex#Op1"},
                "identifiers":{"a":{"target":"ex#Str"},"z":{"target":"ex#Str"}},"type":"resource"},
        "ex#E":{"type":"enum","members":{"B":{"target":"smithy.api#Unit"},"A":{"target":"smithy.api#Unit"}}},
        "ex#Str":{"traits":{"ex#custom":{"k1":true,"k2":[{"a":1.5,"b":1}]}},"type":"string"},
        "ex#S":{"traits":{"smithy.api#documentation":"doc","smithy.api#input":{}},"type":"structure",
                "members":{"z":{"target":"ex#Str"},"a":{"target":"ex#E"}}},
        "ex#Err2":{"type":"structure","traits":{"smithy.api#error":"server"}},
        "ex#Err1":{"type":"structure","traits":{"smithy.api#error":"client"}},
        "ex#Op2":{"type":"operation","input":{"target":"ex#S"}},
        "ex#Op1":{"type":"operation","errors":[{"target":"ex#Err1"},{"target":"ex#Err2"}],
                  "input":{"target":"smithy.api#Unit"}},
        "ex#Svc":{"rename":{"ex#E":"E2","ex#Str":"S2"},"type":"service","version":"1",
                  "operations":[{"target":"ex#Op1"},{"target":"ex#Op2"}]}
    },"metadata":{"a":{"b":null,"y":[3,1]},"z":1},"smithy":"2.0"}"#;

    #[test]
    fn deterministic_bytes() {
        let (a, b) = (load(A), load(B));
        let writer = ModelWriter::new();
        let out = writer.to_string(&a).unwrap();
        assert_eq!(out, writer.to_string(&b).unwrap());
        assert_eq!(out, writer.to_string(&a).unwrap());
        assert!(a.equivalent(&b));
        let pretty = ModelWriter::new().pretty(true).to_string(&a).unwrap();
        assert_eq!(
            pretty,
            ModelWriter::new().pretty(true).to_string(&b).unwrap()
        );
        assert!(pretty.ends_with("}\n") && pretty.contains("\n  \"shapes\": {"));
    }

    #[test]
    fn canonical_form() {
        let out = ModelWriter::new().to_string(&load(A)).unwrap();
        let expected = concat!(
            r#"{"smithy":"2.0","metadata":{"a":{"b":null,"y":[3,1]},"z":1},"shapes":{"#,
            r#""ex#E":{"type":"enum","members":{"B":{"target":"smithy.api#Unit"},"A":{"target":"smithy.api#Unit"}}},"#,
            r#""ex#Err1":{"type":"structure","traits":{"smithy.api#error":"client"}},"#,
            r#""ex#Err2":{"type":"structure","traits":{"smithy.api#error":"server"}},"#,
            r#""ex#Op1":{"type":"operation","input":{"target":"smithy.api#Unit"},"output":{"target":"smithy.api#Unit"},"#,
            r#""errors":[{"target":"ex#Err1"},{"target":"ex#Err2"}]},"#,
            r#""ex#Op2":{"type":"operation","input":{"target":"ex#S"},"output":{"target":"smithy.api#Unit"}},"#,
            r#""ex#R":{"type":"resource","identifiers":{"a":{"target":"ex#Str"},"z":{"target":"ex#Str"}},"#,
            r#""read":{"target":"ex#Op1"},"operations":[{"target":"ex#Op1"},{"target":"ex#Op2"}]},"#,
            r#""ex#S":{"type":"structure","members":{"z":{"target":"ex#Str"},"a":{"target":"ex#E"}},"#,
            r#""traits":{"smithy.api#documentation":"doc","smithy.api#input":{}}},"#,
            r#""ex#Str":{"type":"string","traits":{"ex#custom":{"k1":true,"k2":[{"a":1.5,"b":1}]}}},"#,
            r#""ex#Svc":{"type":"service","version":"1","operations":[{"target":"ex#Op1"},{"target":"ex#Op2"}],"#,
            r#""rename":{"ex#E":"E2","ex#Str":"S2"}}}}"#
        );
        assert_eq!(out, expected);
    }

    #[test]
    fn significant_order_and_values() {
        let base = load(A);
        // Member order is significant.
        let swapped = A.replace(
            r#""z":{"target":"ex#Str"},"a":{"target":"ex#E"}"#,
            r#""a":{"target":"ex#E"},"z":{"target":"ex#Str"}"#,
        );
        assert!(!base.equivalent(&load(&swapped)));
        // Trait array order is significant.
        assert!(!base.equivalent(&load(&A.replace("[3,1]", "[1,3]"))));
        // 1 and 1.0 differ.
        assert!(!base.equivalent(&load(&A.replace(r#""b":1,"#, r#""b":1.0,"#))));
        // Explicit enum values equal to the member name are effectively equal to omission.
        let explicit = A.replace(
            r#""A":{"target":"smithy.api#Unit"}"#,
            r#""A":{"target":"smithy.api#Unit","traits":{"smithy.api#enumValue":"A"}}"#,
        );
        assert!(base.equivalent(&load(&explicit)));
        assert!(!base.equivalent(&load(
            &explicit.replace(r#"enumValue":"A""#, r#"enumValue":"a""#)
        )));
        // Prelude presence matters.
        let json = r#"{"smithy":"2.0","shapes":{"a#B":{"type":"string"}}}"#;
        let bare = ModelLoader::new()
            .disable_prelude()
            .load_str("t", json)
            .unwrap();
        assert!(!bare.equivalent(&load(json)));
    }

    #[test]
    fn round_trip_and_prelude_option() {
        let model = load(A);
        let out = ModelWriter::new().pretty(true).to_string(&model).unwrap();
        assert!(!out.contains("smithy.api#String\""));
        let reloaded = load(&out);
        assert!(model.equivalent(&reloaded));
        assert_eq!(
            ModelWriter::new().to_string(&reloaded).unwrap(),
            ModelWriter::new().to_string(&model).unwrap()
        );

        let with_prelude = ModelWriter::new()
            .include_prelude(true)
            .to_string(&model)
            .unwrap();
        assert!(with_prelude.contains(r#""smithy.api#String":{"type":"string""#));
        assert!(Model::from_json_str("t", &with_prelude).is_err());
        let bare = ModelLoader::new()
            .disable_prelude()
            .load_str("t", &with_prelude)
            .unwrap();
        assert_eq!(bare.len(), model.len());

        let empty = load(r#"{"smithy":"2.0"}"#);
        assert_eq!(
            ModelWriter::new().to_string(&empty).unwrap(),
            r#"{"smithy":"2.0"}"#
        );
    }

    #[test]
    fn io_errors_preserved() {
        struct Failing;
        impl Write for Failing {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("disk full"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        for writer in [ModelWriter::new(), ModelWriter::new().pretty(true)] {
            let err = writer.write(&load(A), Failing).unwrap_err();
            assert!(err.is_io());
            assert_eq!(err.source().unwrap().to_string(), "disk full");
        }
    }
}
