/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! [`ModelAssembler`]: combines Smithy IDL and JSON AST documents, existing models, shapes,
//! metadata, and traits into a validated [`Model`].

use crate::diagnostic::{
    Diagnostic, DiagnosticCode, DiagnosticSet, LoadError, LoadErrorKind, SourceLocation,
};
use crate::idl_loader;
use crate::idl_loader::lower::{self, PendingSyntacticId};
use crate::idl_loader::resolve::DeclarationIndex;
use crate::idl_loader::syntax::IdlDocument;
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

/// Assembles a [`Model`] from any combination of Smithy IDL and JSON AST documents, existing
/// models, shapes built in code, metadata, and traits, then validates it.
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
/// references only need to resolve once everything is assembled. Likewise, a relative shape ID
/// in an IDL document resolves to a shape of that name in its namespace declared by *any*
/// input before falling back to the prelude.
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
    Bytes {
        format: Format,
        source: String,
        bytes: Vec<u8>,
    },
    File {
        format: Format,
        path: PathBuf,
    },
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

/// The representation of a serialized model input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    Json,
    Idl,
}

/// One input after parsing, before it is merged. Every input is parsed first so that IDL
/// relative shape IDs can resolve against declarations from any input.
enum Parsed {
    /// A JSON document, or an input that failed to read or parse.
    Document(Result<LoadedDocument, LoadError>),
    Idl(IdlDocument),
    Model(Model),
    Shape(Box<Shape>),
    Metadata {
        key: String,
        value: Node,
    },
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
    pub fn add_json_slice(self, source: impl Into<String>, input: impl Into<Vec<u8>>) -> Self {
        self.add_bytes(Format::Json, source.into(), input.into())
    }

    /// Adds a JSON AST document from a reader, which is consumed immediately (up to
    /// [`max_input_bytes`](Self::max_input_bytes) plus one byte) and not retained. Set
    /// `max_input_bytes` before adding readers.
    pub fn add_json_reader(self, source: impl Into<String>, reader: impl Read) -> Self {
        self.add_reader(Format::Json, source.into(), reader)
    }

    /// Adds a JSON AST document from a file, which is read when the model is assembled.
    pub fn add_json_file(mut self, path: impl AsRef<Path>) -> Self {
        self.inputs.push(Input::File {
            format: Format::Json,
            path: path.as_ref().to_owned(),
        });
        self
    }

    /// Adds a Smithy IDL (`.smithy`) document. `source` labels its diagnostics and source
    /// locations, for example with a file name.
    ///
    /// Relative shape IDs resolve against the declarations of every input, so an IDL document
    /// may refer to shapes defined in other IDL or JSON documents, existing models, or code.
    ///
    /// ```
    /// use aws_smithy_lang::{Model, ShapeId};
    ///
    /// let common = r#"{"smithy": "2.0", "shapes": {"ex#Name": {"type": "string"}}}"#;
    /// let idl = r#"
    /// $version: "2"
    /// namespace ex
    ///
    /// structure Person {
    ///     @required
    ///     name: Name
    /// }
    /// "#;
    /// let model = Model::assembler()
    ///     .add_json_str("common.json", common)
    ///     .add_idl_str("person.smithy", idl)
    ///     .assemble()
    ///     .into_result()?;
    /// assert!(model.contains_shape(&ShapeId::new("ex#Person")?));
    /// assert_eq!(model.non_prelude_shapes().len(), 2);
    /// # Ok::<_, Box<dyn std::error::Error>>(())
    /// ```
    pub fn add_idl_str(self, source: impl Into<String>, input: impl Into<String>) -> Self {
        self.add_idl_slice(source, input.into().into_bytes())
    }

    /// Adds a Smithy IDL document from bytes, which must be UTF-8.
    pub fn add_idl_slice(self, source: impl Into<String>, input: impl Into<Vec<u8>>) -> Self {
        self.add_bytes(Format::Idl, source.into(), input.into())
    }

    /// Adds a Smithy IDL document from a reader, which is consumed immediately (up to
    /// [`max_input_bytes`](Self::max_input_bytes) plus one byte) and not retained. Set
    /// `max_input_bytes` before adding readers.
    pub fn add_idl_reader(self, source: impl Into<String>, reader: impl Read) -> Self {
        self.add_reader(Format::Idl, source.into(), reader)
    }

    /// Adds a Smithy IDL document from a file, which is read when the model is assembled.
    pub fn add_idl_file(mut self, path: impl AsRef<Path>) -> Self {
        self.inputs.push(Input::File {
            format: Format::Idl,
            path: path.as_ref().to_owned(),
        });
        self
    }

    fn add_bytes(mut self, format: Format, source: String, bytes: Vec<u8>) -> Self {
        self.inputs.push(Input::Bytes {
            format,
            source,
            bytes,
        });
        self
    }

    fn add_reader(mut self, format: Format, source: String, reader: impl Read) -> Self {
        let mut bytes = Vec::new();
        let limit = self.loader.max_input_bytes.saturating_add(1);
        match reader.take(limit).read_to_end(&mut bytes) {
            Ok(_) => self.inputs.push(Input::Bytes {
                format,
                source,
                bytes,
            }),
            Err(err) => self
                .inputs
                .push(Input::Failed(JsonLoader::read_error(&source, err))),
        }
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

    /// The maximum number of bytes in each IDL or JSON input.
    pub fn max_input_bytes(mut self, max: u64) -> Self {
        self.loader.max_input_bytes = max;
        self
    }

    /// The maximum number of shapes in each IDL or JSON document. For IDL, shapes generated
    /// for inline operation input and output count too.
    pub fn max_shapes(mut self, max: usize) -> Self {
        self.loader.max_shapes = max;
        self
    }

    /// The maximum nesting depth of each document. For JSON this counts every object and
    /// array from the root; for IDL it counts the nested arrays and objects (including
    /// structured trait bodies) of each metadata, trait, or property value.
    ///
    /// Values above the supported maximum of 100 are clamped to it, because deeper JSON input
    /// would otherwise be rejected by serde_json's recursion guard (128) as a JSON syntax error
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

    /// Assembles with one more borrowed input, avoiding a copy for the `Model::from_*_slice`
    /// convenience functions.
    pub(crate) fn assemble_with_borrowed(
        self,
        format: Format,
        source: &str,
        input: &[u8],
    ) -> Validated<Model> {
        self.assemble_inner(Some((format, source, input)))
    }

    fn assemble_inner(self, borrowed: Option<(Format, &str, &[u8])>) -> Validated<Model> {
        let Self {
            prelude: with_prelude,
            require_trait_definitions,
            loader,
            inputs,
            validators,
        } = self;

        // Parse every input first; diagnostics are kept with each input and replayed in input
        // order below, so ordering and `max_diagnostics` truncation match a single pass.
        let mut parsed = Vec::with_capacity(inputs.len() + 1);
        let mut pending_traits = Vec::new();
        for input in inputs {
            parsed.push(match input {
                Input::Bytes {
                    format,
                    source,
                    bytes,
                } => parse_bytes(&loader, format, &source, &bytes),
                Input::File { format, path } => match format {
                    Format::Json => {
                        Parsed::Document(loader.parse_file(&path).map(LoadedDocument::from))
                    }
                    Format::Idl => idl_loader::load_file(&loader, &path)
                        .map_or_else(|err| Parsed::Document(Err(err)), Parsed::Idl),
                },
                Input::Failed(err) => Parsed::Document(Err(err)),
                Input::Model(model) => Parsed::Model(model),
                Input::Shape(shape) => Parsed::Shape(shape),
                Input::Metadata { key, value } => Parsed::Metadata { key, value },
                Input::Trait(pending) => {
                    pending_traits.push(pending);
                    continue;
                }
            });
        }
        if let Some((format, source, input)) = borrowed {
            parsed.push(parse_bytes(&loader, format, source, input));
        }
        let index = declaration_index(&parsed);

        let mut merge = Merge {
            prelude: with_prelude,
            shapes: IndexMap::new(),
            metadata: NodeObject::new(),
            diagnostics: DiagnosticSet::with_limit(loader.max_diagnostics),
            failure: None,
            syntactic_ids: Vec::new(),
        };
        for input in parsed {
            match input {
                Parsed::Document(document) => merge.document(document),
                Parsed::Idl(document) => {
                    let index = index.as_ref().expect("built when an IDL input is present");
                    merge.document(lower::lower(&document, index, loader.max_diagnostics));
                }
                Parsed::Model(model) => {
                    let data = model.data();
                    for shape in data.shapes.values() {
                        merge.shape(shape.clone());
                    }
                    for (key, value) in data.metadata.iter() {
                        merge.metadata(key.to_owned(), value.clone());
                    }
                }
                Parsed::Shape(shape) => merge.shape(*shape),
                Parsed::Metadata { key, value } => merge.metadata(key, value),
            }
        }
        for pending in pending_traits {
            merge.apply_trait(pending);
        }

        let Merge {
            shapes,
            metadata,
            mut diagnostics,
            failure,
            syntactic_ids,
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
        for pending in syntactic_ids {
            if let Some(warning) = pending.check(&data) {
                diagnostics.push(warning);
            }
        }
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

/// Parses one in-memory input.
fn parse_bytes(loader: &JsonLoader, format: Format, source: &str, bytes: &[u8]) -> Parsed {
    match format {
        Format::Json => {
            Parsed::Document(loader.parse_slice(source, bytes).map(LoadedDocument::from))
        }
        Format::Idl => idl_loader::load_slice(loader, source, bytes)
            .map_or_else(|err| Parsed::Document(Err(err)), Parsed::Idl),
    }
}

/// Indexes every input's declarations for IDL shape-ID resolution, or returns `None` when no
/// input needs one.
fn declaration_index(parsed: &[Parsed]) -> Option<DeclarationIndex> {
    if !parsed.iter().any(|input| matches!(input, Parsed::Idl(_))) {
        return None;
    }
    let mut index = DeclarationIndex::default();
    for input in parsed {
        match input {
            Parsed::Document(Ok(loaded)) => loaded
                .document
                .shapes
                .values()
                .for_each(|s| index.add_shape(s)),
            Parsed::Idl(document) => index.add_idl_names(document),
            Parsed::Model(model) => model
                .data()
                .shapes
                .values()
                .for_each(|s| index.add_shape(s)),
            Parsed::Shape(shape) => index.add_shape(shape),
            Parsed::Document(Err(_)) | Parsed::Metadata { .. } => {}
        }
    }
    // Identifier targets are relative IDs, so they resolve only once every name is known.
    for input in parsed {
        if let Parsed::Idl(document) = input {
            index.add_idl_resources(document);
        }
    }
    Some(index)
}

/// A successfully loaded document of either representation, plus the side data assembly
/// needs after every input is known.
#[derive(Default)]
pub(crate) struct LoadedDocument {
    pub(crate) document: Document,
    /// Non-error diagnostics, such as ignored IDL control statements.
    pub(crate) diagnostics: DiagnosticSet,
    /// IDL syntactic shape IDs to check once every input is assembled. Always empty for JSON.
    pub(crate) syntactic_ids: Vec<PendingSyntacticId>,
}

impl From<Document> for LoadedDocument {
    fn from(document: Document) -> Self {
        Self {
            document,
            ..Self::default()
        }
    }
}

/// Merge state while combining inputs.
struct Merge {
    prelude: bool,
    shapes: IndexMap<ShapeId, Shape>,
    metadata: NodeObject,
    diagnostics: DiagnosticSet,
    /// The first input-level failure (I/O, syntax, or resource limit).
    failure: Option<LoadErrorKind>,
    /// IDL syntactic shape IDs, checked once every shape is known.
    syntactic_ids: Vec<PendingSyntacticId>,
}

impl Merge {
    fn failed(&mut self, err: LoadError) {
        let (kind, diagnostics) = err.into_parts();
        if !matches!(kind, LoadErrorKind::InvalidModel) && self.failure.is_none() {
            self.failure = Some(kind);
        }
        self.diagnostics.extend(diagnostics);
    }

    fn document(&mut self, document: Result<LoadedDocument, LoadError>) {
        match document {
            Ok(LoadedDocument {
                document: Document { metadata, shapes },
                diagnostics,
                syntactic_ids,
            }) => {
                self.diagnostics.extend(diagnostics);
                self.syntactic_ids.extend(syntactic_ids);
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
    let shape = shapes.get_mut(target.root_str())?;
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
pub(crate) fn merge_nodes(
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
    let Some(target) = data.root(id) else {
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
    let children = |id: &ShapeId| match shapes.get(id) {
        Some(Shape::Resource(r)) => Some(&r.resources[..]),
        _ => None,
    };
    // `false` while on the current path, `true` once fully explored.
    let mut done: HashMap<&ShapeId, bool> = HashMap::new();
    for shape in shapes.values() {
        let start = shape.id();
        if children(start).is_none() || done.contains_key(start) {
            continue;
        }
        let mut stack: Vec<(&ShapeId, usize)> = vec![(start, 0)];
        done.insert(start, false);
        while let Some(&(id, index)) = stack.last() {
            let Some(child) = children(id).unwrap_or(&[]).get(index) else {
                done.insert(id, true);
                stack.pop();
                continue;
            };
            if let Some(top) = stack.last_mut() {
                top.1 += 1;
            }
            match done.get(child) {
                None if children(child).is_some() => {
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
                    let location = shapes[id]
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
