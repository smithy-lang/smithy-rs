/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! [`ModelAssembler`]: combines JSON AST documents, existing models, shapes, metadata, and
//! traits into a validated [`Model`].

use crate::diagnostic::{
    Diagnostic, DiagnosticCode, DiagnosticSet, LoadError, LoadErrorKind, SourceLocation,
};
use crate::json_loader::{Document, JsonLoader, MAX_SUPPORTED_DEPTH};
use crate::knowledge::KnowledgeCache;
use crate::model::{Model, ModelData};
use crate::node::{Node, NodeObject, NodeValue};
use crate::prelude;
use crate::relationships::{for_each_reference, member_reference, DeclaredRef};
use crate::shape::{Shape, TraitMap};
use crate::shape_id::{IntoShapeId, InvalidShapeIdError, ShapeId};
use crate::traits;
use crate::validation::{builtin_validators, DiagnosticSink, Validated, Validator};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Assembles a [`Model`] from any combination of Smithy JSON AST documents, existing models,
/// shapes built in code, metadata, and traits, then validates it.
///
/// Inputs are combined in the order they were added, following Smithy's merge rules:
///
/// - **Shapes.** A shape defined more than once is accepted if every definition is equal
///   (ignoring source locations); the first is kept and a
///   [`DuplicateDefinition`](DiagnosticCode::DuplicateDefinition) note records the others.
///   Different definitions are a [`ShapeConflict`](DiagnosticCode::ShapeConflict) error.
/// - **Metadata.** Equal values for a key are accepted, two arrays are concatenated in input
///   order, and any other difference is a
///   [`MetadataConflict`](DiagnosticCode::MetadataConflict) error.
/// - **Traits** added with [`add_trait`](Self::add_trait) are merged into the target shape or
///   member: equal values are accepted, arrays are concatenated, and any other difference is a
///   [`TraitConflict`](DiagnosticCode::TraitConflict) error.
///
/// The prelude is then attached and the result validated (see the
/// [`validation`](crate::validation) module). Shapes may refer to shapes from any input, so
/// references only need to resolve once everything is assembled.
///
/// ```
/// use aws_smithy_lang::shape::StructureShape;
/// use aws_smithy_lang::Model;
///
/// let operations = r#"{"smithy": "2.0", "shapes": {
///     "ex#GetThing": {"type": "operation", "input": {"target": "ex#GetThingInput"}}
/// }}"#;
/// let input = StructureShape::builder("ex#GetThingInput")
///     .member("id", "smithy.api#String")
///     .build()?;
///
/// let validated = Model::assembler()
///     .add_json_str("operations.json", operations)
///     .add_shape(input)
///     .assemble();
/// let model = validated.into_result()?;
/// assert_eq!(model.non_prelude_shapes().len(), 2);
/// # Ok::<_, Box<dyn std::error::Error>>(())
/// ```
///
/// Documents using `apply` or mixins, and shapes marked `@mixin`, are rejected with
/// [`UnsupportedFeature`](DiagnosticCode::UnsupportedFeature).
pub struct ModelAssembler {
    prelude: bool,
    require_trait_definitions: bool,
    loader: JsonLoader,
    inputs: Vec<Input>,
    validators: Vec<Arc<dyn Validator>>,
}

enum Input {
    Json {
        source: String,
        bytes: Vec<u8>,
    },
    File(PathBuf),
    /// A reader that failed while being consumed.
    Failed(LoadError),
    Model(Model),
    Shape(Box<Shape>),
    Metadata {
        key: String,
        value: Node,
    },
    Trait(PendingTrait),
}

struct PendingTrait {
    target: Result<ShapeId, InvalidShapeIdError>,
    id: Result<ShapeId, InvalidShapeIdError>,
    value: Node,
}

impl Default for ModelAssembler {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for ModelAssembler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelAssembler")
            .field("prelude", &self.prelude)
            .field("require_trait_definitions", &self.require_trait_definitions)
            .field("inputs", &self.inputs.len())
            .field(
                "validators",
                &self.validators.iter().map(|v| v.name()).collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

impl ModelAssembler {
    /// Creates an assembler with default options.
    ///
    /// The defaults inject the Smithy prelude, allow applied traits whose definitions are
    /// absent, and use resource limits far above the largest AWS models.
    pub fn new() -> Self {
        Self {
            prelude: true,
            require_trait_definitions: false,
            loader: JsonLoader::default(),
            inputs: Vec::new(),
            validators: Vec::new(),
        }
    }

    // --- Inputs -----------------------------------------------------------------------------

    /// Adds a JSON AST document. `source` labels its diagnostics and source locations, for
    /// example with a file name.
    pub fn add_json_str(self, source: impl Into<String>, input: impl Into<String>) -> Self {
        self.add_json_slice(source, input.into().into_bytes())
    }

    /// Adds a JSON AST document from bytes.
    pub fn add_json_slice(mut self, source: impl Into<String>, input: impl Into<Vec<u8>>) -> Self {
        self.inputs.push(Input::Json {
            source: source.into(),
            bytes: input.into(),
        });
        self
    }

    /// Adds a JSON AST document from a reader, which is consumed immediately (up to
    /// [`max_input_bytes`](Self::max_input_bytes) plus one byte) and not retained. Set
    /// `max_input_bytes` before adding readers.
    pub fn add_json_reader(mut self, source: impl Into<String>, reader: impl Read) -> Self {
        let source = source.into();
        let mut bytes = Vec::new();
        let limit = self.loader.max_input_bytes.saturating_add(1);
        match reader.take(limit).read_to_end(&mut bytes) {
            Ok(_) => self.inputs.push(Input::Json { source, bytes }),
            Err(err) => self
                .inputs
                .push(Input::Failed(JsonLoader::read_error(&source, err))),
        }
        self
    }

    /// Adds a JSON AST document from a file, which is read when the model is assembled.
    pub fn add_json_file(mut self, path: impl AsRef<Path>) -> Self {
        self.inputs.push(Input::File(path.as_ref().to_owned()));
        self
    }

    /// Adds the shapes and metadata of an existing model. Its prelude and cached knowledge are
    /// not copied; the prelude is attached according to this assembler's options.
    pub fn add_model(mut self, model: &Model) -> Self {
        self.inputs.push(Input::Model(model.clone()));
        self
    }

    /// Adds a shape, such as one built with
    /// [`StructureShape::builder`](crate::shape::StructureShape::builder).
    pub fn add_shape(mut self, shape: impl Into<Shape>) -> Self {
        self.inputs.push(Input::Shape(Box::new(shape.into())));
        self
    }

    /// Adds several shapes.
    pub fn add_shapes(mut self, shapes: impl IntoIterator<Item = impl Into<Shape>>) -> Self {
        self.inputs
            .extend(shapes.into_iter().map(|s| Input::Shape(Box::new(s.into()))));
        self
    }

    /// Adds a metadata entry, merged with metadata from other inputs.
    pub fn put_metadata(mut self, key: impl Into<String>, value: impl Into<Node>) -> Self {
        self.inputs.push(Input::Metadata {
            key: key.into(),
            value: value.into(),
        });
        self
    }

    /// Applies a trait to a shape or member from any input, like a Smithy `apply` statement.
    /// The target must exist once every input is combined, and must not be a prelude shape.
    pub fn add_trait(
        mut self,
        target: impl IntoShapeId,
        trait_id: impl IntoShapeId,
        value: impl Into<Node>,
    ) -> Self {
        self.inputs.push(Input::Trait(PendingTrait {
            target: target.into_shape_id(),
            id: trait_id.into_shape_id(),
            value: value.into(),
        }));
        self
    }

    // --- Options ----------------------------------------------------------------------------

    /// Registers a custom validator. Validators run after the built-in validators, in
    /// registration order.
    pub fn add_validator(mut self, validator: impl Validator + 'static) -> Self {
        self.validators.push(Arc::new(validator));
        self
    }

    /// Does not inject the prelude. References to `smithy.api` shapes then fail to resolve
    /// unless an input defines them.
    pub fn disable_prelude(mut self) -> Self {
        self.prelude = false;
        self
    }

    /// Requires every applied trait to resolve to a trait definition.
    pub fn require_trait_definitions(mut self, require: bool) -> Self {
        self.require_trait_definitions = require;
        self
    }

    /// The maximum number of bytes in each JSON input.
    pub fn max_input_bytes(mut self, max: u64) -> Self {
        self.loader.max_input_bytes = max;
        self
    }

    /// The maximum number of top-level shapes in each JSON document.
    pub fn max_shapes(mut self, max: usize) -> Self {
        self.loader.max_shapes = max;
        self
    }

    /// The maximum JSON nesting depth of each document, counting every object and array from
    /// the root.
    ///
    /// Values above the supported maximum of 100 are clamped to it, because deeper input would
    /// otherwise be rejected by serde_json's recursion guard (128) as a JSON syntax error
    /// instead of a [`ResourceLimit`](DiagnosticCode::ResourceLimit).
    pub fn max_depth(mut self, max: usize) -> Self {
        self.loader.max_depth = max.min(MAX_SUPPORTED_DEPTH);
        self
    }

    /// The maximum number of diagnostics to collect.
    ///
    /// Further diagnostics are dropped and replaced by a single
    /// [`ResourceLimit`](DiagnosticCode::ResourceLimit) error; see
    /// [`DiagnosticSet::is_truncated`]. Assembly fails when the limit is reached.
    pub fn max_diagnostics(mut self, max: usize) -> Self {
        self.loader.max_diagnostics = max;
        self
    }

    // --- Assembly ---------------------------------------------------------------------------

    /// Combines every input, validates the result, and returns the model (if there were no
    /// errors) with every diagnostic.
    pub fn assemble(self) -> Validated<Model> {
        self.assemble_inner(None)
    }

    /// Assembles with one more borrowed JSON input, avoiding a copy for the `Model::from_json_*`
    /// convenience functions.
    pub(crate) fn assemble_with_borrowed(self, source: &str, input: &[u8]) -> Validated<Model> {
        self.assemble_inner(Some((source, input)))
    }

    fn assemble_inner(self, borrowed: Option<(&str, &[u8])>) -> Validated<Model> {
        let Self {
            prelude: with_prelude,
            require_trait_definitions,
            loader,
            inputs,
            validators,
        } = self;
        let mut merge = Merge {
            prelude: with_prelude,
            shapes: IndexMap::new(),
            metadata: NodeObject::new(),
            diagnostics: DiagnosticSet::with_limit(loader.max_diagnostics),
            failure: None,
        };
        let mut pending_traits = Vec::new();
        for input in inputs {
            match input {
                Input::Json { source, bytes } => {
                    merge.document(loader.parse_slice(&source, &bytes))
                }
                Input::File(path) => merge.document(loader.parse_file(&path)),
                Input::Failed(err) => merge.failed(err),
                Input::Model(model) => {
                    let data = model.data();
                    for shape in data.shapes.values() {
                        merge.shape(shape.clone());
                    }
                    for (key, value) in data.metadata.iter() {
                        merge.metadata(key.to_owned(), value.clone());
                    }
                }
                Input::Shape(shape) => merge.shape(*shape),
                Input::Metadata { key, value } => merge.metadata(key, value),
                Input::Trait(pending) => pending_traits.push(pending),
            }
        }
        if let Some((source, input)) = borrowed {
            merge.document(loader.parse_slice(source, input));
        }
        for pending in pending_traits {
            merge.apply_trait(pending);
        }

        let Merge {
            shapes,
            metadata,
            mut diagnostics,
            failure,
            ..
        } = merge;
        if diagnostics.has_errors() {
            return Validated::new(None, diagnostics, failure);
        }

        let data = ModelData {
            shapes,
            metadata,
            prelude: with_prelude.then(prelude::shapes),
            knowledge: KnowledgeCache::default(),
        };
        critical_checks(&data, &mut diagnostics);
        if diagnostics.has_errors() {
            return Validated::new(None, diagnostics, None);
        }

        let model = Model::from_data(data);
        let builtins = builtin_validators(require_trait_definitions);
        for validator in builtins.iter().chain(&validators) {
            let mut sink = DiagnosticSink::new(&mut diagnostics, validator.name());
            validator.validate(&model, &mut sink);
        }
        Validated::new(Some(model), diagnostics, None)
    }
}

/// Merge state while combining inputs.
struct Merge {
    prelude: bool,
    shapes: IndexMap<ShapeId, Shape>,
    metadata: NodeObject,
    diagnostics: DiagnosticSet,
    /// The first input-level failure (I/O, JSON syntax, or resource limit).
    failure: Option<LoadErrorKind>,
}

impl Merge {
    fn failed(&mut self, err: LoadError) {
        let (kind, diagnostics) = err.into_parts();
        if !matches!(kind, LoadErrorKind::InvalidModel) && self.failure.is_none() {
            self.failure = Some(kind);
        }
        self.diagnostics.extend(diagnostics);
    }

    fn document(&mut self, document: Result<Document, LoadError>) {
        match document {
            Ok(Document { metadata, shapes }) => {
                for (key, value) in metadata.into_entries() {
                    self.metadata(key, value);
                }
                for shape in shapes.into_values() {
                    self.shape(shape);
                }
            }
            Err(err) => self.failed(err),
        }
    }

    fn shape(&mut self, shape: Shape) {
        let id = shape.id();
        if self.prelude && prelude::is_prelude_shape(id) {
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::PreludeConflict,
                    format!(
                        "`{id}` cannot be defined in the prelude namespace `{}`",
                        prelude::NAMESPACE
                    ),
                )
                .with_shape_id(id.clone())
                .with_source_location(shape.source_location().clone()),
            );
            return;
        }
        let Some(previous) = self.shapes.get(id) else {
            self.shapes.insert(id.clone(), shape);
            return;
        };
        let (location, first) = (shape.source_location(), previous.source_location());
        if *previous != shape {
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ShapeConflict,
                    format!(
                        "conflicting definitions of `{id}`{}",
                        built_in_code(location, first)
                    ),
                )
                .with_shape_id(id.clone())
                .with_source_location(location.clone())
                .with_related(first.clone()),
            );
        } else if location != first {
            self.diagnostics.push(
                Diagnostic::note(
                    DiagnosticCode::DuplicateDefinition,
                    format!("ignoring duplicate but equivalent definition of `{id}`"),
                )
                .with_shape_id(id.clone())
                .with_source_location(location.clone())
                .with_related(first.clone()),
            );
        }
    }

    fn metadata(&mut self, key: String, value: Node) {
        let Some(previous) = self.metadata.get(&key) else {
            self.metadata.insert(key, value);
            return;
        };
        let merged = merge_nodes(previous, value, |previous, value| {
            Diagnostic::error(
                DiagnosticCode::MetadataConflict,
                format!(
                    "conflicting values for metadata key `{key}`{}",
                    built_in_code(value.source_location(), previous.source_location())
                ),
            )
            .with_source_location(value.source_location().clone())
            .with_related(previous.source_location().clone())
        });
        match merged {
            Ok(Some(merged)) => {
                self.metadata.insert(key, merged);
            }
            Ok(None) => {}
            Err(diagnostic) => self.diagnostics.push(diagnostic),
        }
    }

    fn apply_trait(&mut self, pending: PendingTrait) {
        let (target, id) = match (pending.target, pending.id) {
            (Ok(target), Ok(id)) => (target, id),
            (Err(err), _) | (_, Err(err)) => {
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::InvalidShapeId,
                    format!("cannot apply trait: {err}"),
                ));
                return;
            }
        };
        let location = pending.value.source_location().clone();
        let fail = |code, message: String| {
            Diagnostic::error(code, message)
                .with_shape_id(target.clone())
                .with_source_location(location.clone())
        };
        if id.is_member() {
            self.diagnostics.push(fail(
                DiagnosticCode::InvalidShapeId,
                format!("trait `{id}` is not a root shape ID"),
            ));
            return;
        }
        if id == traits::MIXIN {
            self.diagnostics.push(fail(
                DiagnosticCode::UnsupportedFeature,
                format!("applying `{id}` to `{target}` is not supported"),
            ));
            return;
        }
        if prelude::is_prelude_shape(&target) && self.prelude {
            self.diagnostics.push(fail(
                DiagnosticCode::PreludeConflict,
                format!("cannot apply `{id}` to the prelude shape `{target}`"),
            ));
            return;
        }
        let Some(traits) = trait_map(&mut self.shapes, &target) else {
            self.diagnostics.push(fail(
                DiagnosticCode::UnknownShape,
                format!("trait `{id}` is applied to unknown shape `{target}`"),
            ));
            return;
        };
        let Some(previous) = traits.get(&id) else {
            traits.insert(id, pending.value);
            return;
        };
        let merged = merge_nodes(previous, pending.value, |previous, value| {
            Diagnostic::error(
                DiagnosticCode::TraitConflict,
                format!(
                    "conflicting `{id}` trait values on `{target}`{}",
                    built_in_code(value.source_location(), previous.source_location())
                ),
            )
            .with_shape_id(target.clone())
            .with_source_location(value.source_location().clone())
            .with_related(previous.source_location().clone())
        });
        match merged {
            Ok(Some(merged)) => {
                traits.insert(id, merged);
            }
            Ok(None) => {}
            Err(diagnostic) => self.diagnostics.push(diagnostic),
        }
    }
}

/// The traits of a shape or member, for applying a trait.
fn trait_map<'a>(
    shapes: &'a mut IndexMap<ShapeId, Shape>,
    target: &ShapeId,
) -> Option<&'a mut TraitMap> {
    let shape = shapes.get_mut(target.root().as_str())?;
    match target.member() {
        None => Some(&mut shape.common_mut().traits),
        Some(name) => shape
            .members_mut()
            .into_iter()
            .find(|m| m.name() == name)
            .map(|m| &mut m.traits),
    }
}

/// Merges a repeated metadata or trait value, following Java Smithy's loader:
///
/// - the same value from the same location (the same input added twice) is ignored;
/// - two arrays are concatenated;
/// - an equal value is ignored;
/// - anything else is a conflict.
///
/// Returns `Ok(None)` when `previous` is kept unchanged.
fn merge_nodes(
    previous: &Node,
    value: Node,
    conflict: impl FnOnce(&Node, &Node) -> Diagnostic,
) -> Result<Option<Node>, Diagnostic> {
    if previous == &value && previous.source_location() == value.source_location() {
        return Ok(None);
    }
    match (previous.as_array(), value.as_array()) {
        (Some(previous), Some(_)) => {
            let location = value.source_location().clone();
            let NodeValue::Array(mut elements) = value.into_value() else {
                unreachable!("checked above")
            };
            let mut merged = previous.to_vec();
            merged.append(&mut elements);
            Ok(Some(Node::from(merged).with_source_location(location)))
        }
        _ if previous == &value => Ok(None),
        _ => Err(conflict(previous, &value)),
    }
}

/// Notes which sides of a conflict were created in code, since only loaded values have a
/// location for the diagnostic to show.
fn built_in_code(location: &SourceLocation, other: &SourceLocation) -> &'static str {
    match (location.is_none(), other.is_none()) {
        (true, true) => " (both created in code)",
        (true, false) | (false, true) => " (one created in code)",
        (false, false) => "",
    }
}

/// The checks every [`Model`] guarantees. They run before a model exists, on every shape the
/// inputs defined (the embedded prelude is trusted).
fn critical_checks(data: &ModelData, diagnostics: &mut DiagnosticSet) {
    for shape in data.shapes.values() {
        let source = shape.source_location();
        if let Some(value) = shape.traits().get(&traits::MIXIN) {
            let location = if value.source_location().is_none() {
                source.child(&format!("/traits/{}", traits::MIXIN))
            } else {
                value.source_location().clone()
            };
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::UnsupportedFeature,
                    format!(
                        "`{}` is a mixin (`{}`), which is not supported",
                        shape.id(),
                        traits::MIXIN
                    ),
                )
                .with_shape_id(shape.id().clone())
                .with_source_location(location),
            );
        }
        for_each_reference(shape, |reference| {
            check_reference(data, shape.id(), reference, source, diagnostics)
        });
        for member in shape.members() {
            let reference = member_reference(member);
            check_reference(
                data,
                member.id(),
                reference,
                member.source_location(),
                diagnostics,
            );
        }
    }
    resource_cycles(data, diagnostics);
}

/// Resolves an ordinary reference and checks its target's type. Trait definitions may only be
/// referenced by applying them.
fn check_reference(
    data: &ModelData,
    owner: &ShapeId,
    reference: DeclaredRef<'_>,
    source: &SourceLocation,
    diagnostics: &mut DiagnosticSet,
) {
    let id = reference.target;
    let what = if owner.is_member() {
        format!("member `{owner}`")
    } else {
        reference.kind.describe().to_owned()
    };
    let fail = |code, message: String| {
        Diagnostic::error(code, message)
            .with_shape_id(owner.clone())
            .with_source_location(reference.path.locate(source))
    };
    if id.is_member() {
        diagnostics.push(fail(
            DiagnosticCode::InvalidTarget,
            format!("`{id}` is a member; references must target root shapes"),
        ));
        return;
    }
    let Some(target) = data.root(id.as_str()) else {
        diagnostics.push(fail(
            DiagnosticCode::UnresolvedReference,
            format!("`{id}` does not resolve to a shape"),
        ));
        return;
    };
    if target.traits().contains(&traits::TRAIT) {
        diagnostics.push(fail(
            DiagnosticCode::InvalidTarget,
            format!(
                "{what} targets trait definition `{id}`; trait definitions can only be \
                 referenced by applying them"
            ),
        ));
        return;
    }
    let allowed = reference.kind.allowed_targets();
    let actual = target.shape_type();
    if !allowed.allows(actual) {
        diagnostics.push(fail(
            DiagnosticCode::InvalidTarget,
            format!(
                "{what} must target a {}, but `{id}` is a {actual}",
                allowed.describe()
            ),
        ));
    }
}

/// Reports each resource-containment cycle using an iterative depth-first search.
fn resource_cycles(data: &ModelData, diagnostics: &mut DiagnosticSet) {
    let shapes = &data.shapes;
    let children = |id: &str| match shapes.get(id) {
        Some(Shape::Resource(r)) => Some(&r.resources[..]),
        _ => None,
    };
    // `false` while on the current path, `true` once fully explored.
    let mut done: HashMap<&ShapeId, bool> = HashMap::new();
    for shape in shapes.values() {
        let start = shape.id();
        if children(start.as_str()).is_none() || done.contains_key(start) {
            continue;
        }
        let mut stack: Vec<(&ShapeId, usize)> = vec![(start, 0)];
        done.insert(start, false);
        while let Some(&(id, index)) = stack.last() {
            let Some(child) = children(id.as_str()).unwrap_or(&[]).get(index) else {
                done.insert(id, true);
                stack.pop();
                continue;
            };
            if let Some(top) = stack.last_mut() {
                top.1 += 1;
            }
            match done.get(child) {
                None if children(child.as_str()).is_some() => {
                    done.insert(child, false);
                    stack.push((child, 0));
                }
                Some(false) => {
                    let first = stack.iter().position(|(s, _)| *s == child).unwrap_or(0);
                    let path: Vec<_> = stack[first..]
                        .iter()
                        .map(|(s, _)| s.as_str())
                        .chain(std::iter::once(child.as_str()))
                        .collect();
                    let location = shapes[id.as_str()]
                        .source_location()
                        .child(&format!("/resources/{index}/target"));
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::InvalidShape,
                            format!("resource containment cycle: {}", path.join(" -> ")),
                        )
                        .with_shape_id(id.clone())
                        .with_source_location(location),
                    );
                }
                _ => {}
            }
        }
    }
}
