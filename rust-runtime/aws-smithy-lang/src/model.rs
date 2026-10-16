/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The immutable [`Model`].

use crate::assembler::ModelAssembler;
use crate::diagnostic::LoadError;
use crate::knowledge::{Knowledge, KnowledgeCache};
use crate::node::{Node, NodeObject};
use crate::shape::{
    MemberView, OperationView, ResourceView, ServiceView, Shape, ShapeExpectationError, ShapeRef,
    ShapeType, ShapeView,
};
use crate::shape_id::ShapeId;
use indexmap::{IndexMap, IndexSet};
use std::fmt;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

/// Model storage: owned semantic shapes and metadata.
///
/// `shapes` holds the model's own shapes in assembly order: sources in the order they were
/// added, and each source's shapes in its order. The shared, read-only prelude is referenced
/// rather than copied.
///
/// `knowledge` holds lazily computed derived facts. It only stores owned IDs and indices, never
/// references into the model, and is ignored by the writer and by [`Model::equivalent`].
pub(crate) struct ModelData {
    pub(crate) shapes: IndexMap<ShapeId, Shape>,
    pub(crate) metadata: NodeObject,
    pub(crate) prelude: Option<&'static IndexMap<ShapeId, Shape>>,
    pub(crate) knowledge: KnowledgeCache,
}

impl ModelData {
    /// The number of root shapes, including the prelude.
    pub(crate) fn root_count(&self) -> usize {
        self.shapes.len() + self.prelude.map_or(0, IndexMap::len)
    }

    /// The position of a root shape in [`declarations`](Self::declarations) order.
    /// A member ID resolves to its container's position.
    pub(crate) fn root_position(&self, id: &ShapeId) -> Option<usize> {
        let root = id.root_str();
        if let Some(position) = self.shapes.get_index_of(root) {
            return Some(position);
        }
        Some(self.shapes.len() + self.prelude?.get_index_of(root)?)
    }

    /// The root shape at a position in [`declarations`](Self::declarations) order.
    pub(crate) fn root_at(&self, position: usize) -> &Shape {
        let own = self.shapes.len();
        if position < own {
            &self.shapes[position]
        } else {
            let prelude = self.prelude.expect("root positions are in range");
            &prelude[position - own]
        }
    }

    /// A root shape from the model or the prelude. A member ID resolves to its container.
    pub(crate) fn root(&self, id: &ShapeId) -> Option<&Shape> {
        let root = id.root_str();
        self.shapes.get(root).or_else(|| self.prelude?.get(root))
    }

    pub(crate) fn lookup(&self, id: &ShapeId) -> Option<ShapeRef<'_>> {
        let container = self.root(id)?;
        match id.member() {
            None => Some(ShapeRef::Shape(ShapeView::new(self, container))),
            Some(member) => {
                let member = container.member(member)?;
                Some(ShapeRef::Member(MemberView::new(self, container, member)))
            }
        }
    }

    /// Every root shape: the model's own shapes, then the prelude.
    pub(crate) fn declarations(&self) -> impl Iterator<Item = &Shape> {
        self.shapes
            .values()
            .chain(self.prelude.into_iter().flat_map(IndexMap::values))
    }
}

/// An immutable, structurally valid Smithy model.
///
/// A `Model` is only produced when assembly reports no errors, so every ordinary shape
/// reference in it resolves. Cloning is cheap.
///
/// Load a single JSON AST document with [`Model::from_json_str`] and friends, or use
/// [`Model::assembler`] to combine several documents, existing models, and shapes built in
/// code.
#[derive(Clone)]
pub struct Model {
    data: Arc<ModelData>,
}

impl Model {
    pub(crate) fn from_data(data: ModelData) -> Self {
        Self {
            data: Arc::new(data),
        }
    }

    /// A [`ModelAssembler`] with default options.
    pub fn assembler() -> ModelAssembler {
        ModelAssembler::new()
    }

    /// Loads a model from a JSON AST string with default options. `source` labels
    /// diagnostics, for example with a file name.
    pub fn from_json_str(source: &str, input: &str) -> Result<Model, LoadError> {
        Self::from_json_slice(source, input.as_bytes())
    }

    /// Loads a model from JSON AST bytes with default options.
    pub fn from_json_slice(source: &str, input: &[u8]) -> Result<Model, LoadError> {
        ModelAssembler::new()
            .assemble_with_borrowed(source, input)
            .into_result()
    }

    /// Loads a model from a reader with default options. The reader is consumed during the
    /// call and is not retained.
    pub fn from_json_reader(source: &str, reader: impl Read) -> Result<Model, LoadError> {
        ModelAssembler::new()
            .add_json_reader(source, reader)
            .assemble()
            .into_result()
    }

    /// Loads a model from a file with default options.
    pub fn from_json_file(path: impl AsRef<Path>) -> Result<Model, LoadError> {
        ModelAssembler::new()
            .add_json_file(path)
            .assemble()
            .into_result()
    }

    pub(crate) fn data(&self) -> &ModelData {
        &self.data
    }

    /// Looks up a shape, or a member by its member ID (`ns#Shape$member`).
    ///
    /// Shapes are always looked up by [`ShapeId`]. Use [`ShapeId::from_static`] for IDs known
    /// at compile time, or parse one with [`ShapeId::new`].
    pub fn get_shape(&self, id: &ShapeId) -> Option<ShapeRef<'_>> {
        self.data.lookup(id)
    }

    /// Looks up a shape or member, returning an error if it does not exist.
    pub fn expect_shape(&self, id: &ShapeId) -> Result<ShapeRef<'_>, ShapeExpectationError> {
        self.get_shape(id)
            .ok_or_else(|| ShapeExpectationError::not_found(id))
    }

    /// Returns `true` if a shape or member with this ID exists.
    pub fn contains_shape(&self, id: &ShapeId) -> bool {
        self.get_shape(id).is_some()
    }

    /// Every root shape: the model's own shapes in assembly order, then prelude shapes.
    pub fn shapes(&self) -> impl Iterator<Item = ShapeView<'_>> + '_ {
        let data = &*self.data;
        data.declarations()
            .map(move |shape| ShapeView::new(data, shape))
    }

    /// The model's own root shapes (excluding the prelude), in assembly order.
    pub fn non_prelude_shapes(&self) -> impl ExactSizeIterator<Item = ShapeView<'_>> + '_ {
        let data = &*self.data;
        data.shapes
            .values()
            .map(move |shape| ShapeView::new(data, shape))
    }

    /// Prelude shapes; empty when the prelude is disabled.
    pub fn prelude_shapes(&self) -> impl Iterator<Item = ShapeView<'_>> + '_ {
        let data = &*self.data;
        data.prelude
            .into_iter()
            .flat_map(IndexMap::values)
            .map(move |shape| ShapeView::new(data, shape))
    }

    /// Every root shape ID, in the same order as [`Model::shapes`].
    pub fn shape_ids(&self) -> impl Iterator<Item = &ShapeId> + '_ {
        self.data.declarations().map(Shape::id)
    }

    /// The number of root shapes, including the prelude.
    pub fn len(&self) -> usize {
        self.data.root_count()
    }

    /// Derived, cached knowledge about the model: complete service and resource closures,
    /// operation knowledge, parent lookups, and incoming relationships.
    ///
    /// Indexes are computed on first use and shared by every clone of this model. See the
    /// [`knowledge`](crate::knowledge) module.
    pub fn knowledge(&self) -> Knowledge<'_> {
        Knowledge::new(&self.data)
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
    pub fn shapes_with_trait<'a>(
        &'a self,
        trait_id: &'a ShapeId,
    ) -> impl Iterator<Item = ShapeRef<'a>> + 'a {
        self.shapes().flat_map(move |shape| {
            let root = shape.has_trait(trait_id).then_some(ShapeRef::Shape(shape));
            root.into_iter().chain(
                shape
                    .members()
                    .iter()
                    .filter(move |m| m.has_trait(trait_id))
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

    /// The model's metadata, in assembly order.
    pub fn metadata(&self) -> &NodeObject {
        &self.data.metadata
    }

    /// A metadata value by key.
    pub fn metadata_value(&self, key: &str) -> Option<&Node> {
        self.data.metadata.get(key)
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

    /// Returns `true` if both models have the same semantic content.
    ///
    /// Shapes and metadata are compared as values, independent of assembly order and of the
    /// writer's output order: source locations, version spelling, map and object key order,
    /// and the order of unordered reference sets are ignored. Member order and trait/metadata
    /// array order are significant. An omitted enum value equals an explicit value equal to the
    /// member name, and an omitted operation input/output equals an explicit
    /// `smithy.api#Unit`. Models with and without the prelude are never equivalent.
    pub fn equivalent(&self, other: &Model) -> bool {
        let (a, b) = (&*self.data, &*other.data);
        self.has_prelude() == other.has_prelude()
            && a.metadata == b.metadata
            && a.shapes.len() == b.shapes.len()
            && a.shapes
                .iter()
                .all(|(id, shape)| b.shapes.get(id) == Some(shape))
    }
}

impl fmt::Debug for Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Model")
            .field("shapes", &self.data.shapes.len())
            .field("prelude", &self.has_prelude())
            .field("metadata_keys", &self.data.metadata.len())
            .finish()
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
            .expect_shape(&ShapeId::from_static("ex#Service"))
            .unwrap()
            .expect_service()
            .unwrap();
        assert_eq!(service.version(), Some("2024-01-01"));
        assert_eq!(
            service.renamed(&ShapeId::from_static("ex#Name")),
            Some("ThingName")
        );
        assert_eq!(
            service
                .common_errors()
                .map(ShapeId::as_str)
                .collect::<Vec<_>>(),
            ["ex#Oops"]
        );
        let ops: Vec<_> = service
            .contained_operations()
            .map(|op| (op.input().to_string(), op.output().to_string()))
            .collect();
        assert_eq!(ops, [("ex#GetThingInput".into(), "smithy.api#Unit".into())]);
        assert_eq!(
            service
                .direct_operations()
                .map(ShapeId::as_str)
                .collect::<Vec<_>>(),
            ["ex#GetThing"]
        );
        let op = model
            .expect_shape(&ShapeId::from_static("ex#GetThing"))
            .unwrap()
            .expect_operation()
            .unwrap();
        assert!(op.declared_output().is_none());
        assert_eq!(op.input_shape().id(), "ex#GetThingInput");
        assert_eq!(op.output_shape().shape_type(), ShapeType::Structure);
        let resource = model
            .expect_shape(&ShapeId::from_static("ex#Thing"))
            .unwrap()
            .expect_resource()
            .unwrap();
        assert_eq!(resource.read().unwrap(), "ex#GetThing");
        assert_eq!(resource.identifiers().collect::<Vec<_>>()[0].0, "name");
    }

    #[test]
    fn lookup_and_members() {
        let model = model();
        let member = model
            .expect_shape(&ShapeId::from_static("ex#GetThingInput$name"))
            .unwrap();
        assert!(member.is_member());
        assert_eq!(member.shape_type(), ShapeType::Member);
        let member = member.expect_member().unwrap();
        assert_eq!(member.name(), "name");
        assert_eq!(member.target_shape().id(), "ex#Name");
        assert_eq!(member.container().id(), "ex#GetThingInput");
        assert!(model
            .get_shape(&ShapeId::from_static("ex#GetThingInput$missing"))
            .is_none());
        assert!(model
            .get_shape(&ShapeId::from_static("ex#Name$x"))
            .is_none());

        let input = model
            .expect_shape(&ShapeId::from_static("ex#GetThingInput"))
            .unwrap()
            .expect_shape()
            .unwrap();
        assert_eq!(
            input.members().names().collect::<Vec<_>>(),
            ["name", "tags"]
        );
        assert_eq!(input.members().len(), 2);

        let map = model
            .expect_shape(&ShapeId::from_static("ex#Tags"))
            .unwrap()
            .expect_map()
            .unwrap();
        assert_eq!(map.key().id(), "ex#Tags$key");
        assert_eq!(map.value().target(), "ex#List");
        assert_eq!(map.members().names().collect::<Vec<_>>(), ["key", "value"]);
        let list = model
            .expect_shape(&ShapeId::from_static("ex#List"))
            .unwrap()
            .expect_list()
            .unwrap();
        assert_eq!(list.member().name(), "member");
        assert_eq!(list.members().len(), 1);
        let name = model
            .expect_shape(&ShapeId::from_static("ex#Name"))
            .unwrap();
        assert!(name.members().is_empty());
        assert!(name.members().get_index(0).is_none());

        // Index lookups follow `iter` order and round-trip with `MemberView::index`.
        for shape in [input, map.as_shape(), list.as_shape()] {
            for (index, member) in shape.members().iter().enumerate() {
                assert_eq!(member.index(), index);
                let by_index = shape.members().get_index(index).unwrap();
                assert_eq!(by_index.id(), member.id());
                assert_eq!(shape.member_at(index).unwrap().id(), member.id());
                assert_eq!(shape.shape().member_index(member.name()), Some(index));
                assert_eq!(shape.shape().member_at(index).unwrap().id(), member.id());
            }
            assert!(shape.members().get_index(shape.members().len()).is_none());
        }
        assert_eq!(input.member_at(1).unwrap().name(), "tags");
        assert_eq!(input.shape().member_at(1).unwrap().name(), "tags");
        assert_eq!(input.shape().member_index("tags"), Some(1));
        assert_eq!(input.shape().member_index("missing"), None);
        assert!(member.container().members().get("tags").is_some());
    }

    #[test]
    fn refinement_errors() {
        let model = model();
        let err = model
            .expect_shape(&ShapeId::from_static("ex#Name"))
            .unwrap()
            .expect_service()
            .unwrap_err();
        assert_eq!(err.expected(), Some(ShapeType::Service));
        assert_eq!(err.actual(), Some(ShapeType::String));
        assert_eq!(
            err.to_string(),
            "model.json:27:13: expected `ex#Name` to be a service, but it is a string \
             (at /shapes/ex#Name)"
        );
        let err = model
            .expect_shape(&ShapeId::from_static("ex#Nope"))
            .unwrap_err();
        assert!(err.is_not_found());
        let member = model
            .expect_shape(&ShapeId::from_static("ex#List$member"))
            .unwrap();
        assert!(member.expect_list().is_err());
        assert!(member.expect_shape().is_err());
        assert!(member.as_list().is_none());
        assert!(model
            .expect_shape(&ShapeId::from_static("ex#List"))
            .unwrap()
            .is_list());
    }

    #[test]
    fn enums() {
        let model = model();
        let color = model
            .expect_shape(&ShapeId::from_static("ex#Color"))
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
        // RED omits @enumValue: its effective value is synthesized, its declared value absent.
        let declared: Vec<_> = color
            .members()
            .iter()
            .map(|m| m.declared_enum_value())
            .collect();
        assert_eq!(declared, [None, Some(EnumValue::String("green"))]);
        let level = model
            .expect_shape(&ShapeId::from_static("ex#Level"))
            .unwrap()
            .expect_int_enum()
            .unwrap();
        assert_eq!(
            level.member("LOW").unwrap().enum_value(),
            Some(EnumValue::Int(1))
        );
        assert_eq!(
            level.member("LOW").unwrap().declared_enum_value(),
            Some(EnumValue::Int(1))
        );
        let list = model
            .expect_shape(&ShapeId::from_static("ex#List"))
            .unwrap()
            .expect_list()
            .unwrap();
        assert_eq!(list.member().enum_value(), None);
        assert_eq!(list.member().declared_enum_value(), None);
    }

    #[test]
    fn traits_and_filters() {
        let model = model();
        let service = model
            .expect_shape(&ShapeId::from_static("ex#Service"))
            .unwrap();
        assert_eq!(
            service.get_trait_as::<Documentation>().unwrap(),
            Some(Documentation("A service".into()))
        );
        assert_eq!(service.get_trait_as::<traits::Required>().unwrap(), None);
        let applied: Vec<_> = service.traits().iter().collect();
        assert_eq!(
            applied[0].source_location().pointer(),
            Some("/shapes/ex#Service/traits/smithy.api#documentation")
        );
        let oops = model
            .expect_shape(&ShapeId::from_static("ex#Oops"))
            .unwrap();
        assert_eq!(
            oops.get_trait_as::<traits::ErrorFault>().unwrap(),
            Some(traits::ErrorFault::Client)
        );

        // Trait lookup takes a `ShapeId` on every view; IDs compare by value.
        let doc_id: ShapeId = "smithy.api#documentation".parse().unwrap();
        assert!(!service.has_trait(&traits::REQUIRED));
        assert!(service.has_trait(&doc_id));
        assert!(service.get_trait(&doc_id).is_some());
        assert!(service.traits().contains(&doc_id));
        let service_view = service.expect_shape().unwrap().expect_service().unwrap();
        assert!(service_view.has_trait(&doc_id));
        assert_eq!(service_view.id(), service.id());
        assert_eq!(service_view.shape_type(), ShapeType::Service);
        assert_eq!(service_view.as_shape().id(), service.id());
        assert_eq!(
            model.shapes_with_trait(&doc_id).count(),
            model.shapes_with_trait(&traits::DOCUMENTATION).count()
        );

        let required: Vec<_> = model
            .shapes_with_trait(&traits::REQUIRED)
            .map(|s| s.id().to_string())
            .collect();
        // Prelude members with @required are included as well.
        assert!(required.contains(&"ex#GetThingInput$name".to_string()));
        let errors: Vec<_> = model
            .shapes_with_trait(&traits::ERROR)
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
            Some(&Node::from(Vec::<Node>::new()))
        );
    }

    #[test]
    fn custom_codec() {
        struct Retries(u64);
        impl TraitCodec for Retries {
            const ID: ShapeId = ShapeId::from_static("ex#retries");
            fn from_node(value: &Node) -> Result<Self, TraitDecodeError> {
                value
                    .as_number()
                    .and_then(|n| n.as_u64())
                    .map(Retries)
                    .ok_or_else(|| TraitDecodeError::at(value, "expected a non-negative integer"))
            }
            fn to_node(&self) -> Node {
                Node::from(self.0)
            }
        }
        let json = r#"{"smithy":"2.0","shapes":{
            "ex#A":{"type":"string","traits":{"ex#retries":3}},
            "ex#B":{"type":"string","traits":{"ex#retries":"x"}}}}"#;
        let model = Model::from_json_str("m", json).unwrap();
        let a = model.expect_shape(&ShapeId::from_static("ex#A")).unwrap();
        assert_eq!(a.get_trait_as::<Retries>().unwrap().unwrap().0, 3);
        let err = model
            .expect_shape(&ShapeId::from_static("ex#B"))
            .unwrap()
            .get_trait_as::<Retries>()
            .err()
            .unwrap();
        assert_eq!(err.trait_id().unwrap(), "ex#retries");
        assert_eq!(
            err.source_location().pointer(),
            Some("/shapes/ex#B/traits/ex#retries")
        );
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
        assert!(model.contains_shape(&ShapeId::from_static("smithy.api#String")));

        let bare = Model::assembler()
            .disable_prelude()
            .add_json_str(
                "m",
                r#"{"smithy":"2.0","shapes":{"a#B":{"type":"string"}}}"#,
            )
            .assemble()
            .into_result()
            .unwrap();
        assert!(!bare.has_prelude());
        assert_eq!(bare.len(), 1);
        assert!(!bare.contains_shape(&ShapeId::from_static("smithy.api#String")));
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
        assert_iter(model.shapes_with_trait(&traits::REQUIRED));
        assert_iter(model.applied_trait_ids());
    }
}
