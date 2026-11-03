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
use std::collections::{HashMap, HashSet};
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
///   (ignoring source locations and applied traits, see below); the last one is kept (it
///   provides the source locations) and a
///   [`DuplicateDefinition`](DiagnosticCode::DuplicateDefinition) note records the others.
///   Different definitions are a [`ShapeConflict`](DiagnosticCode::ShapeConflict) error.
/// - **Traits** are applied to a shape or member separately from its definition, as in the
///   [Smithy reference implementation](https://github.com/smithy-lang/smithy), and merged
///   across every definition and every [`add_trait`](Self::add_trait): equal values are
///   accepted, arrays are concatenated, and any other difference is a
///   [`TraitConflict`](DiagnosticCode::TraitConflict) error. So two files that define the same
///   shape with `@tags(["a"])` and `@tags(["b"])` define one shape with both tags. The traits
///   of JSON AST documents, models, and shapes built in code merge first (in input order), then
///   `add_trait` traits, then the traits of IDL documents (in input order), which decides the
///   order of concatenated arrays (the specification leaves that order unspecified). The IDL
///   syntactic sugar for default and enum values (`member: T = value` and enum
///   `NAME = value`) is instead part of the definition, as in the reference implementation:
///   different values in two definitions conflict, and an applied trait with the same ID
///   replaces the value.
/// - **Metadata.** Equal values for a key are accepted, two arrays are concatenated in input
///   order, and any other difference is a
///   [`MetadataConflict`](DiagnosticCode::MetadataConflict) error.
/// - **Shapes, metadata, and models added in code** are merged before any document, whatever
///   order they were added in, as in the reference implementation. Among themselves the last
///   one wins: a later [`put_metadata`](Self::put_metadata) value for a key, or a later shape
///   with the same ID (shapes of [models](Self::add_model) after [shapes](Self::add_shape)),
///   replaces an earlier one without a diagnostic. Shapes in the prelude namespace are ignored,
///   with or without the prelude (see [`disable_prelude`](Self::disable_prelude)). Such a shape is merged with a document that defines it like any other definition, and its
///   traits are merged when a document or [`add_trait`](Self::add_trait) first touches it.
/// - **The prelude namespace.** With the prelude, a document's definition of a shape the
///   prelude defines is merged and compared like any other definition, then replaced by the
///   prelude shape, without a diagnostic; a document may add new shapes to the namespace. Both
///   follow the reference implementation, so a model serialized with its prelude shapes (such as
///   the output of the smithy-build `model` plugin with `includePreludeShapes`) loads. Traits
///   cannot be applied to the prelude's own shapes with [`add_trait`](Self::add_trait), but
///   can be applied to shapes a document added to the namespace, as in the reference
///   implementation.
///
/// The model's shapes keep the order of the inputs that added them.
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
    ///
    /// As in the reference implementation, models are merged before any document and after
    /// every shape added with [`add_shape`](Self::add_shape), whatever order they were added
    /// in. A model's shape replaces a shape with the same ID added in code or by an earlier
    /// model, without a diagnostic, and its metadata is merged like a document's. Shapes in the
    /// prelude namespace are not carried over, including ones a document added to it.
    pub fn add_model(mut self, model: &Model) -> Self {
        self.inputs.push(Input::Model(model.clone()));
        self
    }

    /// Adds a shape, such as one built with
    /// [`StructureShape::builder`](crate::shape::StructureShape::builder).
    ///
    /// As in the reference implementation, shapes added in code are merged before any model or
    /// document, whatever order they were added in. A later shape with the same ID replaces an
    /// earlier one without a diagnostic, and a shape in the prelude namespace is ignored, with
    /// or without the prelude. A document that defines the same shape is merged with it like
    /// any other definition.
    pub fn add_shape(mut self, shape: impl Into<Shape>) -> Self {
        self.inputs.push(Input::Shape(Box::new(shape.into())));
        self
    }

    /// Adds several shapes, like [`add_shape`](Self::add_shape).
    pub fn add_shapes(mut self, shapes: impl IntoIterator<Item = impl Into<Shape>>) -> Self {
        self.inputs
            .extend(shapes.into_iter().map(|s| Input::Shape(Box::new(s.into()))));
        self
    }

    /// Adds a metadata entry, merged with metadata from other inputs.
    ///
    /// As in the reference implementation, metadata added in code is merged before any model or
    /// document, whatever order it was added in, and a later value for the same key replaces an
    /// earlier one without a diagnostic. It then merges with the metadata of models and
    /// documents like any other.
    pub fn put_metadata(mut self, key: impl Into<String>, value: impl Into<Node>) -> Self {
        self.inputs.push(Input::Metadata {
            key: key.into(),
            value: value.into(),
        });
        self
    }

    /// Applies a trait to a shape or member from any input, like a Smithy `apply` statement.
    /// The target must exist once every input is combined, and must not be one of the prelude's
    /// own shapes (unless the prelude is disabled).
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
            created: HashSet::new(),
            input: 0,
            input_order: None,
            definition_traits: HashSet::new(),
            duplicates: Vec::new(),
            deferred_traits: Vec::new(),
            metadata: NodeObject::new(),
            diagnostics: DiagnosticSet::with_limit(loader.max_diagnostics),
            failure: None,
            syntactic_ids: Vec::new(),
        };
        // Like the reference implementation, metadata, shapes, and models added in code are
        // registered before any document, whatever order they were added in. Metadata added in code
        // behaves like a map (the last value of a key wins), and so do shapes built in code and the
        // shapes of models, with shapes of models registered after shapes built in code.
        let mut code_metadata = IndexMap::new();
        let mut code_shapes = Vec::new();
        let mut models = Vec::new();
        let mut documents = Vec::new();
        for (position, input) in parsed.into_iter().enumerate() {
            match input {
                Parsed::Metadata { key, value } => {
                    code_metadata.insert(key, value);
                }
                Parsed::Shape(shape) => code_shapes.push((position, *shape)),
                Parsed::Model(model) => models.push((position, model)),
                document @ (Parsed::Document(_) | Parsed::Idl(_)) => {
                    documents.push((position, document))
                }
            }
        }
        if !code_shapes.is_empty() || !models.is_empty() {
            // Registering them first changes the order shapes are added in; the model keeps
            // the order of its inputs regardless.
            merge.input_order = Some(HashMap::new());
        }
        for (key, value) in code_metadata {
            merge.metadata(key, value);
        }
        for (position, shape) in code_shapes {
            merge.input = position;
            merge.created(shape);
        }
        for (position, model) in models {
            merge.input = position;
            let data = model.data();
            for (key, value) in data.metadata.iter() {
                merge.metadata(key.to_owned(), value.clone());
            }
            for shape in data.shapes.values() {
                merge.created(shape.clone());
            }
        }
        for (position, document) in documents {
            merge.input = position;
            match document {
                Parsed::Document(document) => merge.document(document, false),
                Parsed::Idl(document) => {
                    let index = index.as_ref().expect("built when an IDL input is present");
                    let lowered = lower::lower(&document, index, loader.max_diagnostics);
                    merge.document(lowered, true);
                }
                _ => unreachable!("only documents are collected"),
            }
        }
        for pending in pending_traits {
            merge.apply_trait(pending);
        }
        merge.apply_deferred_traits();
        merge.finish_duplicates();
        merge.drop_prelude_redefinitions();
        merge.restore_input_order();

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
    /// `(target, trait)` pairs that are part of a shape's definition rather than applied
    /// traits; see [`Merge`]. Always empty for JSON.
    pub(crate) definition_traits: HashSet<(ShapeId, ShapeId)>,
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
///
/// Like the loader of the Smithy reference implementation (`software.amazon.smithy:smithy-model`),
/// a shape's *definition* and the traits *applied* to it are merged separately:
///
/// - Every trait in a JSON AST document, every IDL trait annotation and documentation comment,
///   every [`add_trait`](ModelAssembler::add_trait), and every trait of a shape built in code or
///   taken from a model is an applied trait. Applied traits are merged per target (shape or
///   member) with [`merge_nodes`], across every definition of the shape.
/// - The IDL syntactic sugar for default and enum values (`member: T = value` and enum
///   `NAME = value`) is part of the
///   definition, as in the reference implementation, whose IDL loader sets them on the shape
///   builder. An applied trait with the same ID replaces such a value.
///
/// Repeated definitions of a shape are compared only after every input is merged, once every
/// applied trait is known: two definitions are equal when they are equal with every applied
/// trait added to both. So `@tags(["a"])` in one file and `@tags(["b"])` in another define one
/// shape with both tags, while two different `= value` default values conflict. Of equal
/// definitions the last one is kept (it provides the source locations), as in the reference
/// implementation.
///
/// Applied traits merge in the reference implementation's order, which decides the order of
/// concatenated arrays and which value a conflict reports as the previous one: JSON AST traits
/// in document order as documents are merged, then [`add_trait`](ModelAssembler::add_trait)
/// traits, then IDL traits in input order. (The reference implementation resolves every IDL
/// trait as a forward reference once all inputs are loaded.)
///
/// Shapes built in code and the shapes of models are registered before any document (see
/// `assemble_inner`), and follow the reference implementation's rules for such "created"
/// shapes: a later one with the same ID replaces an earlier one, shapes in the prelude namespace
/// are ignored, and a shape only joins the merge once a document defines it or a trait is applied
/// to it. At that point its traits are applied, right after whatever touched it:
///
/// - a trait applied to it (by `add_trait`, a JSON AST document, or an IDL document): that
///   trait first, then the shape's own traits; for a JSON AST definition, the remaining traits
///   of the document follow, and the document's definition counts as the later one;
/// - a definition without applied traits (a JSON AST shape without traits, or any IDL shape,
///   whose traits are deferred): the shape's own traits, and the document's definition counts
///   as the earlier one.
struct Merge {
    prelude: bool,
    /// Every shape: for a shape defined by documents, its kept definition with every applied
    /// trait merged into it; otherwise, a shape built in code or taken from a model.
    shapes: IndexMap<ShapeId, Shape>,
    /// Shapes in [`shapes`](Self::shapes) built in code or taken from a model that nothing has
    /// touched yet; see [`touch_created`](Self::touch_created).
    created: HashSet<ShapeId>,
    /// The index of the input being merged.
    input: usize,
    /// When inputs are not merged in the order they were added (because shapes built in code
    /// or models are registered first), the `(input, sequence)` at which each shape was first
    /// added, used by [`restore_input_order`](Self::restore_input_order).
    input_order: Option<HashMap<ShapeId, (usize, usize)>>,
    /// The `(target, trait)` pairs of [`shapes`](Self::shapes) that still hold a definition
    /// trait, as opposed to an applied one.
    definition_traits: HashSet<(ShapeId, ShapeId)>,
    /// Later definitions of shapes in [`shapes`](Self::shapes), holding only their definition
    /// traits, compared by [`finish_duplicates`](Self::finish_duplicates).
    duplicates: Vec<Shape>,
    /// Applied traits of IDL documents, merged by
    /// [`apply_deferred_traits`](Self::apply_deferred_traits).
    deferred_traits: Vec<DeferredTrait>,
    metadata: NodeObject,
    diagnostics: DiagnosticSet,
    /// The first input-level failure (I/O, syntax, or resource limit).
    failure: Option<LoadErrorKind>,
    /// IDL syntactic shape IDs, checked once every shape is known.
    syntactic_ids: Vec<PendingSyntacticId>,
}

/// Applied traits as `(target, trait, value, index)`, where `index` is the trait's position
/// among its target's traits.
type AppliedTraits = Vec<(ShapeId, ShapeId, Node, usize)>;

impl Merge {
    fn failed(&mut self, err: LoadError) {
        let (kind, diagnostics) = err.into_parts();
        if !matches!(kind, LoadErrorKind::InvalidModel) && self.failure.is_none() {
            self.failure = Some(kind);
        }
        self.diagnostics.extend(diagnostics);
    }

    /// Merges a document. `defer_traits` is `true` for IDL documents, whose applied traits are
    /// merged after every other input.
    fn document(&mut self, document: Result<LoadedDocument, LoadError>, defer_traits: bool) {
        match document {
            Ok(LoadedDocument {
                document:
                    Document {
                        metadata,
                        shapes,
                        first_traits,
                    },
                diagnostics,
                syntactic_ids,
                definition_traits,
            }) => {
                self.diagnostics.extend(diagnostics);
                self.syntactic_ids.extend(syntactic_ids);
                for (key, value) in metadata.into_entries() {
                    self.metadata(key, value);
                }
                for mut shape in shapes.into_values() {
                    if defer_traits {
                        // A first definition gets its traits back in source order.
                        let first = !self.shapes.contains_key(shape.id());
                        let applied = take_applied_traits(&mut shape, &definition_traits);
                        self.deferred_traits.extend(applied.into_iter().map(
                            |(target, id, value, position)| DeferredTrait {
                                target,
                                id,
                                value,
                                position: first.then_some(position),
                            },
                        ));
                    }
                    let first_trait = first_traits.get(shape.id()).cloned();
                    self.definition(shape, &definition_traits, first_trait);
                }
            }
            Err(err) => self.failed(err),
        }
    }

    /// Registers a shape built in code or taken from a model. As in the reference implementation,
    /// it replaces an earlier one with the same ID, and shapes in the prelude namespace are
    /// ignored.
    fn created(&mut self, shape: Shape) {
        if prelude::is_prelude_shape(shape.id()) {
            return;
        }
        let id = shape.id().clone();
        self.created.insert(id.clone());
        self.record_input_order(&id);
        self.shapes.insert(id, shape);
    }

    /// Records where a shape first appeared, if inputs are merged out of order.
    fn record_input_order(&mut self, id: &ShapeId) {
        if let Some(order) = &mut self.input_order {
            let sequence = order.len();
            order.entry(id.clone()).or_insert((self.input, sequence));
        }
    }

    /// Puts shapes back in the order of the inputs that first added them, which the model
    /// promises; registering shapes built in code and models first changed it.
    fn restore_input_order(&mut self) {
        if let Some(order) = self.input_order.take() {
            self.shapes.sort_by_cached_key(|id, _| order[id]);
        }
    }

    /// If `id` is a shape built in code or taken from a model that nothing has touched yet, takes
    /// its traits so they can be applied in the order the reference implementation applies them,
    /// and returns them. From then on the shape is merged like a document definition.
    fn touch_created(&mut self, id: &str) -> Option<AppliedTraits> {
        if !self.created.remove(id) {
            return None;
        }
        let shape = self
            .shapes
            .get_mut(id)
            .expect("created shapes are in `shapes`");
        Some(take_applied_traits(shape, &HashSet::new()))
    }

    /// Adds one definition of a shape from a document. `definition_traits` lists which of its
    /// traits are part of the definition; every other trait is applied. `first_trait` is the
    /// shape's first applied trait in document order as `(target, trait)`, when that is not
    /// the [canonical one](crate::json_loader::canonical_first_trait).
    fn definition(
        &mut self,
        mut shape: Shape,
        definition_traits: &HashSet<(ShapeId, ShapeId)>,
        first_trait: Option<(ShapeId, ShapeId)>,
    ) {
        let id = shape.id().clone();
        if let Some(created_traits) = self.touch_created(id.as_str()) {
            let mut applied = take_applied_traits(&mut shape, definition_traits);
            if applied.is_empty() {
                // Defined before any of its traits are applied: this definition is the
                // earlier one, and the shape built in code the later one.
                let kept = self
                    .shapes
                    .get_mut(&id)
                    .expect("created shapes are in `shapes`");
                let created = std::mem::replace(kept, shape);
                self.add_definition_traits(&id, definition_traits);
                self.merge_all(created_traits);
                self.duplicates.push(created);
            } else {
                // The reference implementation applies a JSON AST shape's traits in document order
                // before defining it: the first one touches the shape built in code, whose traits
                // follow, and this definition is the later one.
                let first = first_trait_index(&applied, first_trait.as_ref());
                let (target, trait_id, value, _) = applied.remove(first);
                self.merge_trait(&target, trait_id, value, None);
                self.merge_all(created_traits);
                self.merge_all(applied);
                self.duplicates.push(shape);
            }
            return;
        }
        if !self.shapes.contains_key(&id) {
            self.add_definition_traits(&id, definition_traits);
            self.record_input_order(&id);
            self.shapes.insert(id, shape);
            return;
        }
        // A later definition: its applied traits apply to the shape (and so to every
        // definition), and the definition itself is compared once every input is merged.
        let applied = take_applied_traits(&mut shape, definition_traits);
        self.merge_all(applied);
        self.duplicates.push(shape);
    }

    /// Records the definition traits of the kept definition of `id`.
    fn add_definition_traits(
        &mut self,
        id: &ShapeId,
        definition_traits: &HashSet<(ShapeId, ShapeId)>,
    ) {
        self.definition_traits.extend(
            definition_traits
                .iter()
                .filter(|(target, _)| target.root_str() == id.as_str())
                .cloned(),
        );
    }

    /// Merges applied traits in order. A target that only a later definition has is reported
    /// as a conflict when the definitions are compared, so it is skipped here.
    fn merge_all(&mut self, traits: AppliedTraits) {
        for (target, id, value, _) in traits {
            self.merge_trait(&target, id, value, None);
        }
    }

    /// Merges a trait applied by `add_trait` or an IDL document. If it touches a shape built in
    /// code, that shape's own traits are applied right after it, as in the reference
    /// implementation.
    fn merge_applied_trait(
        &mut self,
        target: &ShapeId,
        id: ShapeId,
        value: Node,
        position: Option<usize>,
    ) -> bool {
        let created_traits = self.touch_created(target.root_str());
        let merged = self.merge_trait(target, id, value, position);
        if let Some(created_traits) = created_traits {
            self.merge_all(created_traits);
        }
        merged
    }

    /// Merges the applied traits of IDL documents, after every other input and `add_trait`.
    fn apply_deferred_traits(&mut self) {
        for deferred in std::mem::take(&mut self.deferred_traits) {
            // The target is a shape or member of the same document, so it exists.
            let DeferredTrait {
                target,
                id,
                value,
                position,
            } = deferred;
            self.merge_applied_trait(&target, id, value, position);
        }
    }

    /// Compares every repeated definition with the kept one, now that every applied trait is known.
    /// Like the reference implementation, an equal definition replaces the kept one (so the last of
    /// equal definitions provides the source locations), with a note; a different one conflicts and
    /// is dropped.
    fn finish_duplicates(&mut self) {
        for duplicate in std::mem::take(&mut self.duplicates) {
            let id = duplicate.id().clone();
            let kept = &self.shapes[&id];
            let duplicate = with_applied_traits(duplicate, kept, &self.definition_traits);
            let (location, kept_location) = (duplicate.source_location(), kept.source_location());
            if *kept != duplicate {
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ShapeConflict,
                        format!(
                            "conflicting definitions of `{id}`{}",
                            built_in_code(location, kept_location)
                        ),
                    )
                    .with_shape_id(id.clone())
                    .with_source_location(location.clone())
                    .with_related(kept_location.clone()),
                );
                continue;
            }
            if location != kept_location {
                self.diagnostics.push(
                    Diagnostic::note(
                        DiagnosticCode::DuplicateDefinition,
                        format!("ignoring duplicate but equivalent definition of `{id}`"),
                    )
                    .with_shape_id(id.clone())
                    .with_source_location(location.clone())
                    .with_related(kept_location.clone()),
                );
            }
            let kept = self
                .shapes
                .get_mut(&id)
                .expect("duplicates have a kept shape");
            adopt_locations(kept, &duplicate, &self.definition_traits);
        }
    }

    /// Drops document definitions of shapes the prelude defines: as in the reference
    /// implementation, the prelude replaces them, once they have been merged and compared like
    /// any other definition. Shapes a document adds to the prelude namespace are kept.
    fn drop_prelude_redefinitions(&mut self) {
        if self.prelude {
            let prelude = prelude::shapes();
            self.shapes.retain(|id, _| !prelude.contains_key(id));
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
        // As in the reference implementation, only the prelude's own shapes are immutable here;
        // shapes a document added to the namespace accept traits applied in code.
        if self.prelude && prelude::shapes().contains_key(target.root_str()) {
            self.diagnostics.push(fail(
                DiagnosticCode::PreludeConflict,
                format!("cannot apply `{id}` to the prelude shape `{target}`"),
            ));
            return;
        }
        if !self.merge_applied_trait(&target, id.clone(), pending.value, None)
            && !self.has_target(&target)
        {
            self.diagnostics.push(fail(
                DiagnosticCode::UnknownShape,
                format!("trait `{id}` is applied to unknown shape `{target}`"),
            ));
        }
    }

    /// Returns `true` if any definition has `target` (a root shape or member).
    fn has_target(&self, target: &ShapeId) -> bool {
        let defines = |shape: &Shape| {
            shape.id().as_str() == target.root_str()
                && target
                    .member()
                    .is_none_or(|name| shape.member(name).is_some())
        };
        self.shapes.get(target.root_str()).is_some_and(defines)
            || self.duplicates.iter().any(defines)
    }

    /// Merges an applied trait into the first definition of its target. Returns `false` if
    /// that definition does not have the target.
    ///
    /// A new trait is added at `position` (clamped), or last. Positions only keep a first IDL
    /// definition's traits in source order; they do not affect merging.
    fn merge_trait(
        &mut self,
        target: &ShapeId,
        id: ShapeId,
        value: Node,
        position: Option<usize>,
    ) -> bool {
        let Some(traits) = trait_map(&mut self.shapes, target) else {
            return false;
        };
        // An applied trait replaces a definition trait (an IDL value assignment).
        if self.definition_traits.remove(&(target.clone(), id.clone())) {
            traits.insert(id, value);
            return true;
        }
        let Some(previous) = traits.get(&id) else {
            match position {
                Some(position) => {
                    traits.shift_insert(position.min(traits.len()), id, value);
                }
                None => {
                    traits.insert(id, value);
                }
            }
            return true;
        };
        let merged = merge_nodes(previous, value, |previous, value| {
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
        true
    }
}

/// An applied trait of an IDL document, merged after every other input.
struct DeferredTrait {
    target: ShapeId,
    id: ShapeId,
    value: Node,
    /// The trait's index on its target, for a shape's first definition.
    position: Option<usize>,
}

/// Removes the applied traits of `shape` and its members, returning them as
/// `(target, trait, value, index)` in declaration order, where `index` is the trait's position
/// among its target's traits. Traits listed in `definition_traits` stay.
fn take_applied_traits(
    shape: &mut Shape,
    definition_traits: &HashSet<(ShapeId, ShapeId)>,
) -> AppliedTraits {
    let mut applied = Vec::new();
    let mut split = |target: &ShapeId, traits: &mut TraitMap| {
        for (index, (id, value)) in std::mem::take(traits).into_iter().enumerate() {
            if definition_traits.contains(&(target.clone(), id.clone())) {
                traits.insert(id, value);
            } else {
                applied.push((target.clone(), id, value, index));
            }
        }
    };
    let root = shape.id().clone();
    split(&root, &mut shape.common_mut().traits);
    for member in shape.members_mut() {
        let target = member.id.clone();
        split(&target, &mut member.traits);
    }
    applied
}

/// The index in `applied` (from [`take_applied_traits`]) of a JSON AST shape's first trait
/// in document order: `first`, or else the canonical first trait, which is the first member
/// trait or else the first shape trait (see
/// [`canonical_first_trait`](crate::json_loader::canonical_first_trait)).
fn first_trait_index(applied: &AppliedTraits, first: Option<&(ShapeId, ShapeId)>) -> usize {
    let found = match first {
        Some((target, id)) => applied
            .iter()
            .position(|(t, i, _, _)| t == target && i == id),
        None => applied.iter().position(|(target, ..)| target.is_member()),
    };
    found.unwrap_or(0)
}

/// Gives `kept` the source locations of an equal later definition, which the reference
/// implementation keeps instead: the shape's and its members' locations, and the values (with their
/// locations) of the traits that are still definition traits.
fn adopt_locations(
    kept: &mut Shape,
    duplicate: &Shape,
    definition_traits: &HashSet<(ShapeId, ShapeId)>,
) {
    kept.common_mut().source = duplicate.source_location().clone();
    for member in kept.members_mut() {
        let Some(other) = duplicate.member(member.name()) else {
            continue;
        };
        member.source = other.source.clone();
        for (id, value) in &other.traits {
            if definition_traits.contains(&(member.id.clone(), id.clone())) {
                if let Some(slot) = member.traits.get_mut(id) {
                    *slot = value.clone();
                }
            }
        }
    }
}

/// A later definition (holding only its definition traits) with the applied traits of the
/// shape added, so it can be compared with `first`. The applied traits of a target are the
/// traits of `first` that are not definition traits; members that `first` lacks keep only
/// their own traits.
fn with_applied_traits(
    mut duplicate: Shape,
    first: &Shape,
    definition_traits: &HashSet<(ShapeId, ShapeId)>,
) -> Shape {
    let apply = |target: &ShapeId, from: &TraitMap, to: &mut TraitMap| {
        for (id, value) in from {
            if !definition_traits.contains(&(target.clone(), id.clone())) {
                to.insert(id.clone(), value.clone());
            }
        }
    };
    apply(
        first.id(),
        &first.common().traits,
        &mut duplicate.common_mut().traits,
    );
    for member in duplicate.members_mut() {
        if let Some(first_member) = first.member(member.name()) {
            let target = member.id.clone();
            apply(&target, &first_member.traits, &mut member.traits);
        }
    }
    duplicate
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

/// Merges a repeated metadata or trait value, following Smithy's reference implementation
/// loader:
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
