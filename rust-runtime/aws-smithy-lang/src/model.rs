/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The immutable [`Model`].

use crate::ast::{Document, ShapeDecl};
use crate::diagnostic::LoadError;
use crate::loader::ModelLoader;
use crate::node::{Node, NodeObject};
use crate::shape::{
    MemberView, OperationView, ResourceView, ServiceView, ShapeExpectationError, ShapeRef,
    ShapeType, ShapeView,
};
use crate::shape_id::ShapeId;
use crate::{prelude, validate};
use indexmap::IndexSet;
use std::fmt;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, OnceLock};

/// `smithy.api#Unit`, the normalized target of an omitted operation input or output.
pub(crate) fn unit_id() -> &'static ShapeId {
    static UNIT: OnceLock<ShapeId> = OnceLock::new();
    UNIT.get_or_init(|| ShapeId::new("smithy.api#Unit").expect("valid shape ID"))
}

/// Model storage.
///
/// `document` is the declaration layer: exactly what the document declared. In `0.1` the
/// resolved (effective) view is computed directly from declarations because `apply` and
/// mixins are rejected. A separate resolution index will be added alongside them.
pub(crate) struct ModelData {
    pub(crate) document: Document,
    pub(crate) prelude: Option<&'static Document>,
}

impl ModelData {
    /// A root shape declaration from the document or the prelude.
    pub(crate) fn root(&self, id: &str) -> Option<&ShapeDecl> {
        self.document
            .shapes
            .get(id)
            .or_else(|| self.prelude?.shapes.get(id))
    }

    pub(crate) fn lookup(&self, id: &str) -> Option<ShapeRef<'_>> {
        match id.split_once('$') {
            None => Some(ShapeRef::Shape(ShapeView::new(self, self.root(id)?))),
            Some((root, member)) => {
                let container = self.root(root)?;
                let decl = container.member(member)?;
                Some(ShapeRef::Member(MemberView::new(self, container, decl)))
            }
        }
    }

    pub(crate) fn declarations(&self) -> impl Iterator<Item = &ShapeDecl> {
        self.document
            .shapes
            .values()
            .chain(self.prelude.into_iter().flat_map(|p| p.shapes.values()))
    }
}

/// An immutable, structurally valid Smithy model.
///
/// A `Model` is only returned when loading produced no errors, so every ordinary shape
/// reference in it resolves. Cloning is cheap.
#[derive(Clone)]
pub struct Model {
    data: Arc<ModelData>,
}

impl Model {
    /// Loads a model from a JSON AST string with default options. `source` labels
    /// diagnostics, for example with a file name.
    pub fn from_json_str(source: &str, input: &str) -> Result<Model, LoadError> {
        ModelLoader::new().load_str(source, input)
    }

    /// Loads a model from JSON AST bytes with default options.
    pub fn from_json_slice(source: &str, input: &[u8]) -> Result<Model, LoadError> {
        ModelLoader::new().load_slice(source, input)
    }

    /// Loads a model from a reader with default options. The reader is consumed during the
    /// call and is not retained.
    pub fn from_json_reader(source: &str, reader: impl Read) -> Result<Model, LoadError> {
        ModelLoader::new().load_reader(source, reader)
    }

    /// Loads a model from a file with default options.
    pub fn from_json_file(path: impl AsRef<Path>) -> Result<Model, LoadError> {
        ModelLoader::new().load_file(path)
    }

    pub(crate) fn data(&self) -> &ModelData {
        &self.data
    }

    /// Looks up a shape or member by absolute ID.
    pub fn get_shape(&self, id: impl AsRef<str>) -> Option<ShapeRef<'_>> {
        self.data.lookup(id.as_ref())
    }

    /// Looks up a shape or member, returning an error if it does not exist.
    pub fn expect_shape(&self, id: impl AsRef<str>) -> Result<ShapeRef<'_>, ShapeExpectationError> {
        let id = id.as_ref();
        self.get_shape(id)
            .ok_or_else(|| ShapeExpectationError::not_found(id))
    }

    /// Returns `true` if a shape or member with this ID exists.
    pub fn contains_shape(&self, id: impl AsRef<str>) -> bool {
        self.get_shape(id).is_some()
    }

    /// Every root shape: document declarations in source order, then prelude shapes.
    pub fn shapes(&self) -> impl Iterator<Item = ShapeView<'_>> + '_ {
        let data = &*self.data;
        data.declarations()
            .map(move |decl| ShapeView::new(data, decl))
    }

    /// Root shapes declared by the document, in source order.
    pub fn non_prelude_shapes(&self) -> impl ExactSizeIterator<Item = ShapeView<'_>> + '_ {
        let data = &*self.data;
        data.document
            .shapes
            .values()
            .map(move |decl| ShapeView::new(data, decl))
    }

    /// Prelude shapes; empty when the prelude is disabled.
    pub fn prelude_shapes(&self) -> impl Iterator<Item = ShapeView<'_>> + '_ {
        let data = &*self.data;
        data.prelude
            .into_iter()
            .flat_map(|p| p.shapes.values())
            .map(move |decl| ShapeView::new(data, decl))
    }

    /// Every root shape ID, in the same order as [`Model::shapes`].
    pub fn shape_ids(&self) -> impl Iterator<Item = &ShapeId> + '_ {
        self.data.declarations().map(|decl| &decl.id)
    }

    /// The number of root shapes, including the prelude.
    pub fn len(&self) -> usize {
        self.data.document.shapes.len() + self.data.prelude.map_or(0, |p| p.shapes.len())
    }

    /// Returns `true` if the model has no root shapes (only possible without the prelude).
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns `true` if the Smithy prelude was injected.
    pub fn has_prelude(&self) -> bool {
        self.data.prelude.is_some()
    }

    /// Root shapes of the given type. [`ShapeType::Member`] yields nothing; members are
    /// reached through their containers.
    pub fn shapes_by_type(
        &self,
        shape_type: ShapeType,
    ) -> impl Iterator<Item = ShapeView<'_>> + '_ {
        self.shapes().filter(move |s| s.shape_type() == shape_type)
    }

    /// Root shapes and members that have `trait_id` applied.
    pub fn shapes_with_trait(&self, trait_id: &str) -> impl Iterator<Item = ShapeRef<'_>> + '_ {
        let trait_id = trait_id.to_owned();
        self.shapes().flat_map(move |shape| {
            let root = shape.has_trait(&trait_id).then_some(ShapeRef::Shape(shape));
            let trait_id = trait_id.clone();
            root.into_iter().chain(
                shape
                    .members()
                    .iter()
                    .filter(move |m| m.has_trait(&trait_id))
                    .map(ShapeRef::Member),
            )
        })
    }

    /// Every service shape.
    pub fn services(&self) -> impl Iterator<Item = ServiceView<'_>> + '_ {
        self.shapes().filter_map(|s| s.as_service())
    }

    /// Every operation shape.
    pub fn operations(&self) -> impl Iterator<Item = OperationView<'_>> + '_ {
        self.shapes().filter_map(|s| s.as_operation())
    }

    /// Every resource shape.
    pub fn resources(&self) -> impl Iterator<Item = ResourceView<'_>> + '_ {
        self.shapes().filter_map(|s| s.as_resource())
    }

    /// The document's metadata.
    pub fn metadata(&self) -> &NodeObject {
        &self.data.document.metadata
    }

    /// A metadata value by key.
    pub fn metadata_value(&self, key: &str) -> Option<&Node> {
        self.data.document.metadata.get(key)
    }

    /// Every distinct trait ID applied to any shape or member, in order of first use.
    pub fn applied_trait_ids(&self) -> impl Iterator<Item = &ShapeId> + '_ {
        let mut ids = IndexSet::new();
        for shape in self.shapes() {
            ids.extend(shape.traits().ids());
            for member in shape.members().iter() {
                ids.extend(member.traits().ids());
            }
        }
        ids.into_iter()
    }
}

impl fmt::Debug for Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Model")
            .field("shapes", &self.data.document.shapes.len())
            .field("prelude", &self.has_prelude())
            .field("metadata_keys", &self.data.document.metadata.len())
            .finish()
    }
}

impl ModelLoader {
    /// Loads a model from a JSON AST string.
    pub fn load_str(&self, source: &str, input: &str) -> Result<Model, LoadError> {
        self.build(self.parse_str(source, input)?)
    }

    /// Loads a model from JSON AST bytes.
    pub fn load_slice(&self, source: &str, input: &[u8]) -> Result<Model, LoadError> {
        self.build(self.parse_slice(source, input)?)
    }

    /// Loads a model from a reader, consuming it during the call.
    pub fn load_reader(&self, source: &str, reader: impl Read) -> Result<Model, LoadError> {
        self.build(self.parse_reader(source, reader)?)
    }

    /// Loads a model from a file.
    pub fn load_file(&self, path: impl AsRef<Path>) -> Result<Model, LoadError> {
        self.build(self.parse_file(path.as_ref())?)
    }

    fn build(&self, document: Document) -> Result<Model, LoadError> {
        let prelude = self.prelude.then(prelude::prelude);
        let diagnostics = validate::validate(&document, prelude, self);
        if diagnostics.has_errors() {
            return Err(LoadError::invalid_model(diagnostics));
        }
        Ok(Model {
            data: Arc::new(ModelData { document, prelude }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shape::EnumValue;
    use crate::traits::{self, Documentation, TraitCodec, TraitDecodeError};

    const MODEL: &str = r#"{
        "smithy": "2.0",
        "metadata": {"suppressions": []},
        "shapes": {
            "ex#Service": {
                "type": "service", "version": "2024-01-01",
                "operations": [{"target": "ex#GetThing"}],
                "resources": [{"target": "ex#Thing"}],
                "errors": [{"target": "ex#Oops"}],
                "rename": {"ex#Name": "ThingName"},
                "traits": {"smithy.api#documentation": "A service"}
            },
            "ex#GetThing": {
                "type": "operation",
                "input": {"target": "ex#GetThingInput"},
                "errors": [{"target": "ex#Oops"}]
            },
            "ex#GetThingInput": {
                "type": "structure",
                "members": {
                    "name": {"target": "ex#Name", "traits": {"smithy.api#required": {}}},
                    "tags": {"target": "ex#Tags"}
                },
                "traits": {"smithy.api#input": {}}
            },
            "ex#Oops": {"type": "structure", "traits": {"smithy.api#error": "client"}},
            "ex#Name": {"type": "string"},
            "ex#Tags": {"type": "map", "key": {"target": "smithy.api#String"}, "value": {"target": "ex#List"}},
            "ex#List": {"type": "list", "member": {"target": "smithy.api#Integer"}},
            "ex#Color": {"type": "enum", "members": {
                "RED": {"target": "smithy.api#Unit"},
                "GREEN": {"target": "smithy.api#Unit", "traits": {"smithy.api#enumValue": "green"}}
            }},
            "ex#Level": {"type": "intEnum", "members": {
                "LOW": {"target": "smithy.api#Unit", "traits": {"smithy.api#enumValue": 1}}
            }},
            "ex#Thing": {
                "type": "resource",
                "identifiers": {"name": {"target": "ex#Name"}},
                "read": {"target": "ex#GetThing"}
            }
        }
    }"#;

    fn model() -> Model {
        Model::from_json_str("model.json", MODEL).unwrap()
    }

    #[test]
    fn readme_style_inspection() {
        let model = model();
        let service = model
            .expect_shape("ex#Service")
            .unwrap()
            .expect_service()
            .unwrap();
        assert_eq!(service.version(), Some("2024-01-01"));
        assert_eq!(service.renamed("ex#Name"), Some("ThingName"));
        assert_eq!(
            service.errors().map(ShapeId::as_str).collect::<Vec<_>>(),
            ["ex#Oops"]
        );
        let ops: Vec<_> = service
            .operations()
            .map(|id| {
                let op = model.expect_shape(id).unwrap().expect_operation().unwrap();
                (op.input().to_string(), op.output().to_string())
            })
            .collect();
        assert_eq!(ops, [("ex#GetThingInput".into(), "smithy.api#Unit".into())]);
        let op = model
            .expect_shape("ex#GetThing")
            .unwrap()
            .expect_operation()
            .unwrap();
        assert!(op.declared_output().is_none());
        assert_eq!(op.input_shape().id(), "ex#GetThingInput");
        assert_eq!(op.output_shape().shape_type(), ShapeType::Structure);
        let resource = model
            .expect_shape("ex#Thing")
            .unwrap()
            .expect_resource()
            .unwrap();
        assert_eq!(resource.read().unwrap(), "ex#GetThing");
        assert_eq!(resource.identifiers().collect::<Vec<_>>()[0].0, "name");
    }

    #[test]
    fn lookup_and_members() {
        let model = model();
        let member = model.expect_shape("ex#GetThingInput$name").unwrap();
        assert!(member.is_member());
        assert_eq!(member.shape_type(), ShapeType::Member);
        let member = member.expect_member().unwrap();
        assert_eq!(member.name(), "name");
        assert_eq!(member.target_shape().id(), "ex#Name");
        assert_eq!(member.container().id(), "ex#GetThingInput");
        assert!(model.get_shape("ex#GetThingInput$missing").is_none());
        assert!(model.get_shape("ex#Name$x").is_none());

        let input = model
            .expect_shape("ex#GetThingInput")
            .unwrap()
            .expect_shape()
            .unwrap();
        assert_eq!(
            input.members().names().collect::<Vec<_>>(),
            ["name", "tags"]
        );
        assert_eq!(input.members().len(), 2);

        let map = model.expect_shape("ex#Tags").unwrap().expect_map().unwrap();
        assert_eq!(map.key().id(), "ex#Tags$key");
        assert_eq!(map.value().target(), "ex#List");
        assert_eq!(map.members().names().collect::<Vec<_>>(), ["key", "value"]);
        let list = model
            .expect_shape("ex#List")
            .unwrap()
            .expect_list()
            .unwrap();
        assert_eq!(list.member().name(), "member");
        assert_eq!(list.members().len(), 1);
        let name = model.expect_shape("ex#Name").unwrap();
        assert!(name.members().is_empty());
        assert!(member.container().members().get("tags").is_some());
    }

    #[test]
    fn refinement_errors() {
        let model = model();
        let err = model
            .expect_shape("ex#Name")
            .unwrap()
            .expect_service()
            .unwrap_err();
        assert_eq!(err.expected(), Some(ShapeType::Service));
        assert_eq!(err.actual(), Some(ShapeType::String));
        assert_eq!(
            err.to_string(),
            "expected `ex#Name` to be a service, but it is a string"
        );
        let err = model.expect_shape("ex#Nope").unwrap_err();
        assert!(err.is_not_found());
        let member = model.expect_shape("ex#List$member").unwrap();
        assert!(member.expect_list().is_err());
        assert!(member.expect_shape().is_err());
        assert!(member.as_list().is_none());
        assert!(model.expect_shape("ex#List").unwrap().is_list());
    }

    #[test]
    fn enums() {
        let model = model();
        let color = model
            .expect_shape("ex#Color")
            .unwrap()
            .expect_enum()
            .unwrap();
        let values: Vec<_> = color
            .members()
            .iter()
            .map(|m| m.enum_value().unwrap())
            .collect();
        assert_eq!(
            values,
            [EnumValue::String("RED"), EnumValue::String("green")]
        );
        let level = model
            .expect_shape("ex#Level")
            .unwrap()
            .expect_int_enum()
            .unwrap();
        assert_eq!(
            level.member("LOW").unwrap().enum_value(),
            Some(EnumValue::Int(1))
        );
        let list = model
            .expect_shape("ex#List")
            .unwrap()
            .expect_list()
            .unwrap();
        assert_eq!(list.member().enum_value(), None);
    }

    #[test]
    fn traits_and_filters() {
        let model = model();
        let service = model.expect_shape("ex#Service").unwrap();
        assert_eq!(
            service.get_trait_as::<Documentation>().unwrap(),
            Some(Documentation("A service".into()))
        );
        assert_eq!(service.get_trait_as::<traits::Required>().unwrap(), None);
        let applied: Vec<_> = service.traits().iter().collect();
        assert_eq!(
            applied[0].source().pointer(),
            Some("/shapes/ex#Service/traits/smithy.api#documentation")
        );
        let oops = model.expect_shape("ex#Oops").unwrap();
        assert_eq!(
            oops.get_trait_as::<traits::Error>().unwrap(),
            Some(traits::Error::Client)
        );

        let required: Vec<_> = model
            .shapes_with_trait(traits::REQUIRED)
            .map(|s| s.id().to_string())
            .collect();
        // Prelude members with @required are included as well.
        assert!(required.contains(&"ex#GetThingInput$name".to_string()));
        let errors: Vec<_> = model
            .shapes_with_trait(traits::ERROR)
            .filter(|s| !s.id().namespace().starts_with("smithy"))
            .map(|s| s.id().to_string())
            .collect();
        assert_eq!(errors, ["ex#Oops"]);
        assert_eq!(model.services().count(), 1);
        assert_eq!(model.operations().count(), 1);
        assert_eq!(model.resources().count(), 1);
        assert_eq!(model.shapes_by_type(ShapeType::IntEnum).count(), 1);
        assert_eq!(model.shapes_by_type(ShapeType::Member).count(), 0);
        let ids: Vec<_> = model.applied_trait_ids().map(ShapeId::as_str).collect();
        assert!(ids.starts_with(&[
            "smithy.api#documentation",
            "smithy.api#input",
            "smithy.api#required"
        ]));
        assert_eq!(
            ids.iter().filter(|i| **i == "smithy.api#required").count(),
            1
        );
        assert_eq!(
            model.metadata_value("suppressions"),
            Some(&Node::Array(vec![]))
        );
    }

    #[test]
    fn custom_codec() {
        struct Retries(u64);
        impl TraitCodec for Retries {
            const ID: &'static str = "ex#retries";
            fn from_node(value: &Node) -> Result<Self, TraitDecodeError> {
                value
                    .as_number()
                    .and_then(|n| n.as_u64())
                    .map(Retries)
                    .ok_or_else(|| TraitDecodeError::new("expected a non-negative integer"))
            }
            fn to_node(&self) -> Node {
                Node::from(self.0)
            }
        }
        let json = r#"{"smithy":"2.0","shapes":{
            "ex#A":{"type":"string","traits":{"ex#retries":3}},
            "ex#B":{"type":"string","traits":{"ex#retries":"x"}}}}"#;
        let model = Model::from_json_str("m", json).unwrap();
        let a = model.expect_shape("ex#A").unwrap();
        assert_eq!(a.get_trait_as::<Retries>().unwrap().unwrap().0, 3);
        let err = model
            .expect_shape("ex#B")
            .unwrap()
            .get_trait_as::<Retries>()
            .err()
            .unwrap();
        assert_eq!(err.trait_id(), Some("ex#retries"));
    }

    #[test]
    fn prelude_ordering_and_iteration() {
        let model = model();
        let ids: Vec<_> = model.shape_ids().map(ShapeId::as_str).collect();
        assert_eq!(ids[0], "ex#Service");
        assert_eq!(model.non_prelude_shapes().len(), 10);
        assert_eq!(model.len(), 10 + 141);
        assert_eq!(model.prelude_shapes().count(), 141);
        assert!(model.prelude_shapes().all(|s| s.is_prelude()));
        assert!(model.non_prelude_shapes().all(|s| !s.is_prelude()));
        assert!(ids[10..].iter().all(|id| id.starts_with("smithy.api#")));
        assert!(model.contains_shape("smithy.api#String"));

        let bare = ModelLoader::new()
            .disable_prelude()
            .load_str(
                "m",
                r#"{"smithy":"2.0","shapes":{"a#B":{"type":"string"}}}"#,
            )
            .unwrap();
        assert!(!bare.has_prelude());
        assert_eq!(bare.len(), 1);
        assert!(!bare.contains_shape("smithy.api#String"));
    }

    #[test]
    fn send_sync() {
        fn assert<T: Send + Sync>() {}
        assert::<Model>();
        assert::<ShapeView<'static>>();
        assert::<ShapeRef<'static>>();
        assert::<MemberView<'static>>();
        assert::<crate::shape::MembersView<'static>>();
        assert::<crate::shape::Traits<'static>>();
        assert::<ServiceView<'static>>();
        assert::<ShapeExpectationError>();
        fn assert_iter<I: Iterator + Send + Sync>(_: I) {}
        let model = model();
        assert_iter(model.shapes());
        assert_iter(model.shapes_with_trait("x"));
        assert_iter(model.applied_trait_ids());
    }
}
