/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Smithy 2.0 JSON AST serialization.
//!
//! By default the writer preserves the order a model was built in: shapes in assembly order,
//! and traits, metadata, nested object keys, service `rename`, resource `identifiers` and
//! `properties`, and reference lists in insertion order. A model loaded from JSON therefore
//! writes its maps in the order they appeared, and a model built in code writes them in the
//! order they were added.
//!
//! [`AstWriter::canonical`] instead sorts everything Smithy treats as unordered (shape IDs,
//! trait IDs, metadata and nested object keys, `rename`, `identifiers`, `properties`, and
//! reference sets), so semantically equal models produce identical bytes.
//!
//! In both modes, member order and array order are preserved, operation `input`/`output` are
//! written explicitly (normalized to `smithy.api#Unit`), optional empty properties are
//! omitted, and source locations are never written.
//!
//! Serialization borrows the model and does not clone trait values.
//!
//! Note: This is currently using `serde` to write out the model. In the future we might
//! base this off of the SDKs existing schema serde implementation, but we need to figure
//! out what we want the dependency graph to look like before we make that switch.

use crate::diagnostic::WriteError;
use crate::model::Model;
use crate::node::{Node, NodeSer, NodeValue};
use crate::shape::{MemberShape, Shape, TraitMap};
use crate::shape_id::ShapeId;
use indexmap::IndexMap;
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};
use std::io::Write;

/// Writes a [`Model`] as Smithy 2.0 JSON AST.
///
/// ```
/// use aws_smithy_lang::{AstWriter, Model};
///
/// let model = Model::from_json_str("m", r#"{"smithy": "2", "shapes": {
///     "ex#B": {"type": "string"}, "ex#A": {"type": "operation"}
/// }}"#)?;
/// // Shapes keep the order they were loaded in.
/// let json = AstWriter::new().to_string(&model)?;
/// assert_eq!(
///     json,
///     r#"{"smithy":"2.0","shapes":{"ex#B":{"type":"string"},"ex#A":{"type":"operation","input":{"target":"smithy.api#Unit"},"output":{"target":"smithy.api#Unit"}}}}"#
/// );
/// // Canonical output sorts them.
/// let json = AstWriter::new().canonical(true).to_string(&model)?;
/// assert!(json.find("ex#A").unwrap() < json.find("ex#B").unwrap());
/// # Ok::<_, Box<dyn std::error::Error>>(())
/// ```
///
/// Mixins are not supported yet, so there is no mixin option. When they are, the writer will
/// offer a choice between preserving mixins (writing the introduced members and traits plus
/// mixin references) and flattening them, rather than baking either in.
#[derive(Debug, Clone, Default)]
pub struct AstWriter {
    pretty: bool,
    include_prelude: bool,
    canonical: bool,
}

impl AstWriter {
    /// A writer producing compact, insertion-ordered output without prelude shapes.
    pub fn new() -> Self {
        Self::default()
    }

    /// Produces indented output (two spaces, trailing newline) instead of compact output.
    pub fn pretty(mut self, pretty: bool) -> Self {
        self.pretty = pretty;
        self
    }

    /// Also writes prelude shapes. Output written this way can only be reloaded with
    /// [`ModelAssembler::disable_prelude`](crate::ModelAssembler::disable_prelude).
    pub fn include_prelude(mut self, include: bool) -> Self {
        self.include_prelude = include;
        self
    }

    /// Sorts every unordered collection, so semantically equal models produce identical
    /// output regardless of how they were built. Member and array order are still preserved.
    pub fn canonical(mut self, canonical: bool) -> Self {
        self.canonical = canonical;
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
            sort: self.canonical,
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

/// Sorts `entries` by key when writing canonical output.
fn ordered<K: Ord, V>(mut entries: Vec<(K, V)>, sort: bool) -> Vec<(K, V)> {
    if sort {
        entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    }
    entries
}

struct DocumentSer<'a> {
    model: &'a Model,
    include_prelude: bool,
    sort: bool,
}

impl Serialize for DocumentSer<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let data = self.model.data();
        let mut shapes: Vec<&Shape> = data.shapes.values().collect();
        if self.include_prelude {
            shapes.extend(data.prelude.into_iter().flat_map(IndexMap::values));
        }
        if self.sort {
            shapes.sort_unstable_by(|a, b| a.id().cmp(b.id()));
        }

        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("smithy", "2.0")?;
        let metadata = &data.metadata;
        if !metadata.is_empty() {
            map.serialize_entry("metadata", &ObjectSer(metadata.iter().collect(), self.sort))?;
        }
        if !shapes.is_empty() {
            let shapes = Entries(
                shapes
                    .into_iter()
                    .map(|shape| {
                        let ser = ShapeSer {
                            shape,
                            sort: self.sort,
                        };
                        (shape.id().as_str(), ser)
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

/// A node, with nested object keys sorted when `sort` is set.
struct NodeOut<'a>(&'a Node, bool);

impl Serialize for NodeOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let sort = self.1;
        match self.0.value() {
            NodeValue::Array(elements) if sort => {
                let mut seq = serializer.serialize_seq(Some(elements.len()))?;
                for element in elements {
                    seq.serialize_element(&NodeOut(element, sort))?;
                }
                seq.end()
            }
            NodeValue::Object(object) if sort => {
                ObjectSer(object.iter().collect(), sort).serialize(serializer)
            }
            _ => NodeSer(self.0).serialize(serializer),
        }
    }
}

/// Object entries, sorted by key when `sort` is set.
struct ObjectSer<'a>(Vec<(&'a str, &'a Node)>, bool);

impl Serialize for ObjectSer<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let sort = self.1;
        let entries = ordered(self.0.clone(), sort);
        let mut map = serializer.serialize_map(Some(entries.len()))?;
        for (key, value) in entries {
            map.serialize_entry(key, &NodeOut(value, sort))?;
        }
        map.end()
    }
}

struct TraitsSer<'a>(&'a TraitMap, bool);

impl Serialize for TraitsSer<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let sort = self.1;
        let entries = ordered(
            self.0
                .iter()
                .map(|(id, value)| (id.as_str(), value))
                .collect(),
            sort,
        );
        let mut map = serializer.serialize_map(Some(entries.len()))?;
        for (key, value) in entries {
            map.serialize_entry(key, &NodeOut(value, sort))?;
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

/// An unordered set of shape references, sorted when `sort` is set.
struct RefSet<'a>(Vec<&'a ShapeId>);

impl<'a> RefSet<'a> {
    fn new(ids: &'a [ShapeId], sort: bool) -> Self {
        let mut ids: Vec<_> = ids.iter().collect();
        if sort {
            ids.sort_unstable();
        }
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

fn ref_map(map: &IndexMap<String, ShapeId>, sort: bool) -> Entries<'_, Ref<'_>> {
    Entries(ordered(
        map.iter().map(|(k, v)| (k.as_str(), Ref(v))).collect(),
        sort,
    ))
}

struct MemberSer<'a>(&'a MemberShape, bool);

impl Serialize for MemberSer<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let member = self.0;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("target", member.target.as_str())?;
        if !member.traits.is_empty() {
            map.serialize_entry("traits", &TraitsSer(&member.traits, self.1))?;
        }
        map.end()
    }
}

struct ShapeSer<'a> {
    shape: &'a Shape,
    sort: bool,
}

impl Serialize for ShapeSer<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let sort = self.sort;
        let member = |m| MemberSer(m, sort);
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("type", self.shape.shape_type().as_str())?;
        match self.shape {
            Shape::List(list) => map.serialize_entry("member", &member(&list.member))?,
            Shape::Map(m) => {
                map.serialize_entry("key", &member(&m.key))?;
                map.serialize_entry("value", &member(&m.value))?;
            }
            Shape::Service(service) => {
                if let Some(version) = &service.version {
                    map.serialize_entry("version", version)?;
                }
                for (key, ids) in [
                    ("operations", &service.operations),
                    ("resources", &service.resources),
                    ("errors", &service.errors),
                ] {
                    if !ids.is_empty() {
                        map.serialize_entry(key, &RefSet::new(ids, sort))?;
                    }
                }
                if !service.rename.is_empty() {
                    let rename = service
                        .rename
                        .iter()
                        .map(|(id, name)| (id.as_str(), name.as_str()))
                        .collect();
                    map.serialize_entry("rename", &Entries(ordered(rename, sort)))?;
                }
            }
            Shape::Operation(operation) => {
                map.serialize_entry("input", &Ref(operation.input()))?;
                map.serialize_entry("output", &Ref(operation.output()))?;
                if !operation.errors.is_empty() {
                    map.serialize_entry("errors", &RefSet::new(&operation.errors, sort))?;
                }
            }
            Shape::Resource(resource) => {
                if !resource.identifiers.is_empty() {
                    map.serialize_entry("identifiers", &ref_map(&resource.identifiers, sort))?;
                }
                if !resource.properties.is_empty() {
                    map.serialize_entry("properties", &ref_map(&resource.properties, sort))?;
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
                        map.serialize_entry(key, &RefSet::new(ids, sort))?;
                    }
                }
            }
            shape => {
                if let Some(members) = shape.named_members().filter(|m| !m.is_empty()) {
                    let members = Entries(
                        members
                            .iter()
                            .map(|(name, m)| (name.as_str(), member(m)))
                            .collect(),
                    );
                    map.serialize_entry("members", &members)?;
                }
            }
        }
        let traits = &self.shape.common().traits;
        if !traits.is_empty() {
            map.serialize_entry("traits", &TraitsSer(traits, sort))?;
        }
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembler::ModelAssembler;
    use crate::shape::{ServiceShape, StructureShape};
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
    fn canonical_bytes_are_order_independent() {
        let (a, b) = (load(A), load(B));
        let writer = AstWriter::new().canonical(true);
        let out = writer.to_string(&a).unwrap();
        assert_eq!(out, writer.to_string(&b).unwrap());
        assert_eq!(out, writer.to_string(&a).unwrap());
        assert!(a.equivalent(&b));
        let pretty = writer.clone().pretty(true).to_string(&a).unwrap();
        assert_eq!(pretty, writer.pretty(true).to_string(&b).unwrap());
        assert!(pretty.ends_with("}\n") && pretty.contains("\n  \"shapes\": {"));
        // Default output follows each model's own order, so it differs, but it is still
        // deterministic and the models remain equivalent.
        let default = AstWriter::new();
        assert_ne!(
            default.to_string(&a).unwrap(),
            default.to_string(&b).unwrap()
        );
        assert_eq!(
            default.to_string(&a).unwrap(),
            default.to_string(&a).unwrap()
        );
    }

    #[test]
    fn default_output_preserves_insertion_order() {
        let out = AstWriter::new().to_string(&load(A)).unwrap();
        let expected = concat!(
            r#"{"smithy":"2.0","metadata":{"z":1,"a":{"y":[3,1],"b":null}},"shapes":{"#,
            r#""ex#Svc":{"type":"service","version":"1","operations":[{"target":"ex#Op2"},{"target":"ex#Op1"}],"#,
            r#""rename":{"ex#Str":"S2","ex#E":"E2"}},"#,
            r#""ex#Op1":{"type":"operation","input":{"target":"smithy.api#Unit"},"output":{"target":"smithy.api#Unit"},"#,
            r#""errors":[{"target":"ex#Err2"},{"target":"ex#Err1"}]},"#,
            r#""ex#Op2":{"type":"operation","input":{"target":"ex#S"},"output":{"target":"smithy.api#Unit"}},"#,
            r#""ex#Err1":{"type":"structure","traits":{"smithy.api#error":"client"}},"#,
            r#""ex#Err2":{"type":"structure","traits":{"smithy.api#error":"server"}},"#,
            r#""ex#S":{"type":"structure","members":{"z":{"target":"ex#Str"},"a":{"target":"ex#E"}},"#,
            r#""traits":{"smithy.api#input":{},"smithy.api#documentation":"doc"}},"#,
            r#""ex#Str":{"type":"string","traits":{"ex#custom":{"k2":[{"b":1,"a":1.5}],"k1":true}}},"#,
            r#""ex#E":{"type":"enum","members":{"B":{"target":"smithy.api#Unit"},"A":{"target":"smithy.api#Unit"}}},"#,
            r#""ex#R":{"type":"resource","identifiers":{"z":{"target":"ex#Str"},"a":{"target":"ex#Str"}},"#,
            r#""read":{"target":"ex#Op1"},"operations":[{"target":"ex#Op2"},{"target":"ex#Op1"}]}}}"#
        );
        assert_eq!(out, expected);
        // Reloading the output preserves the order again.
        assert_eq!(AstWriter::new().to_string(&load(&out)).unwrap(), out);
    }

    #[test]
    fn manually_built_maps_keep_insertion_order() {
        let tags = crate::node::NodeObject::from_iter([("zeta", 1i64), ("alpha", 2i64)]);
        let service = ServiceShape::builder("ex#Svc")
            .rename("ex#Z", "Zed")
            .rename("ex#A", "Ay")
            .add_trait("ex#tags", tags)
            .build()
            .unwrap();
        let model = ModelAssembler::new()
            .add_shape(StructureShape::builder("ex#Z").build().unwrap())
            .add_shape(StructureShape::builder("ex#A").build().unwrap())
            .add_shape(service)
            .put_metadata("z", 1i64)
            .put_metadata("a", 2i64)
            .assemble()
            .into_result()
            .unwrap();
        let out = AstWriter::new().to_string(&model).unwrap();
        assert_eq!(
            out,
            concat!(
                r#"{"smithy":"2.0","metadata":{"z":1,"a":2},"shapes":{"#,
                r#""ex#Z":{"type":"structure"},"ex#A":{"type":"structure"},"#,
                r#""ex#Svc":{"type":"service","rename":{"ex#Z":"Zed","ex#A":"Ay"},"#,
                r#""traits":{"ex#tags":{"zeta":1,"alpha":2}}}}}"#
            )
        );
        let canonical = AstWriter::new().canonical(true).to_string(&model).unwrap();
        assert!(
            canonical.contains(r#""rename":{"ex#A":"Ay","ex#Z":"Zed"}"#),
            "{canonical}"
        );
        assert!(canonical.contains(r#"{"alpha":2,"zeta":1}"#), "{canonical}");
    }

    #[test]
    fn canonical_form() {
        let out = AstWriter::new()
            .canonical(true)
            .to_string(&load(A))
            .unwrap();
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
        let bare = ModelAssembler::new()
            .disable_prelude()
            .add_json_str("t", json)
            .assemble()
            .into_result()
            .unwrap();
        assert!(!bare.equivalent(&load(json)));
    }

    #[test]
    fn round_trip_and_prelude_option() {
        let model = load(A);
        let out = AstWriter::new().pretty(true).to_string(&model).unwrap();
        assert!(!out.contains("smithy.api#String\""));
        let reloaded = load(&out);
        assert!(model.equivalent(&reloaded));
        for writer in [AstWriter::new(), AstWriter::new().canonical(true)] {
            assert_eq!(
                writer.to_string(&reloaded).unwrap(),
                writer.to_string(&model).unwrap()
            );
        }

        let with_prelude = AstWriter::new()
            .include_prelude(true)
            .to_string(&model)
            .unwrap();
        assert!(with_prelude.contains(r#""smithy.api#String":{"type":"string""#));
        assert!(Model::from_json_str("t", &with_prelude).is_err());
        let bare = ModelAssembler::new()
            .disable_prelude()
            .add_json_str("t", with_prelude.clone())
            .assemble()
            .into_result()
            .unwrap();
        assert_eq!(bare.len(), model.len());

        let empty = load(r#"{"smithy":"2.0"}"#);
        assert_eq!(
            AstWriter::new().to_string(&empty).unwrap(),
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
        for writer in [AstWriter::new(), AstWriter::new().pretty(true)] {
            let err = writer.write(&load(A), Failing).unwrap_err();
            assert!(err.is_io());
            assert_eq!(err.source().unwrap().to_string(), "disk full");
        }
    }
}
