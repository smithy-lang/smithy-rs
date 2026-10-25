/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Structured model diagnostics and operation errors.

use crate::shape_id::ShapeId;
use std::fmt;
use std::sync::Arc;

/// The severity of a [`Diagnostic`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Informational.
    Note,
    /// A problem that does not prevent loading.
    Warning,
    /// A problem that prevents a model from being returned.
    Error,
}

/// A stable category for a [`Diagnostic`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticCode {
    /// Input could not be read.
    Io,
    /// Input is not valid JSON.
    JsonSyntax,
    /// An object contains the same key more than once.
    DuplicateKey,
    /// A configured resource limit was exceeded.
    ResourceLimit,
    /// The `smithy` version is not supported.
    UnsupportedVersion,
    /// The document uses a construct this version of the library does not support.
    UnsupportedFeature,
    /// A shape ID is malformed or not absolute.
    InvalidShapeId,
    /// A required property is missing.
    MissingProperty,
    /// A property is not allowed in this position.
    UnknownProperty,
    /// A property has the wrong JSON type or an invalid value.
    InvalidProperty,
    /// The `type` of a shape is not known.
    UnknownShapeType,
    /// A user document redefines a prelude shape.
    PreludeConflict,
    /// A shape reference does not resolve.
    UnresolvedReference,
    /// A shape reference resolves to a shape of the wrong kind.
    InvalidTarget,
    /// Two shapes or members differ only by case.
    CaseConflict,
    /// An applied trait resolves to a shape that is not a trait definition, or a required
    /// definition is missing.
    InvalidTrait,
    /// A shape violates a structural rule (empty union, duplicate enum value, cycles, ...).
    InvalidShape,
    /// An unordered set of shape references (such as service `operations`) lists the same
    /// shape more than once.
    DuplicateReference,
    /// Two sources define the same shape differently.
    ShapeConflict,
    /// Two sources define the same metadata key with conflicting values.
    MetadataConflict,
    /// The same trait is applied to a shape more than once with conflicting values.
    TraitConflict,
    /// An identical shape was defined more than once; the duplicate was ignored.
    DuplicateDefinition,
    /// A trait was applied to a shape that does not exist.
    UnknownShape,
    /// Reported by a custom [`Validator`](crate::validation::Validator).
    Custom,
}

/// Where a shape, member, trait, or node came from.
///
/// Values loaded from a model record the caller-supplied source label (such as a file name),
/// the 1-based line and column, and an [RFC 6901](https://www.rfc-editor.org/rfc/rfc6901) JSON
/// Pointer to the value. Values created in code use [`SourceLocation::none`].
///
/// The line and column are those of the object key that introduces the value (for example
/// `"MyShape": {`), or of the value itself for strings and objects in arrays. When a key
/// contains escape sequences, an object uses the position of its first key; other values
/// without a position of their own (such as a number in an array) use the position of the
/// closest enclosing value. Locations derived after loading, such as
/// the location of a shape reference, use the line of the enclosing shape or member; their
/// pointer still identifies the exact value.
///
/// `Display` renders `file:line:column (pointer)`:
///
/// ```text
/// model.json:12:9 (/shapes/example#Shape/members/name)
/// ```
///
/// Locations are cheap to clone and are ignored by equality of nodes, shapes, and models.
#[derive(Clone, Default)]
pub struct SourceLocation(Repr);

#[derive(Clone, Default)]
enum Repr {
    #[default]
    None,
    Data(Arc<LocationData>),
    /// One entry of a table shared by every value of a node tree, so loading a large trait
    /// value allocates one table rather than one location per nested value.
    Table(Arc<LocationTable>, u32),
    /// A position recorded while parsing, before the value's source and pointer are known.
    /// The parser replaces these before a value escapes, and public accessors treat them as
    /// [`SourceLocation::none`].
    Pending(Position),
}

/// A 1-based line and column (in characters) within a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Position {
    pub(crate) line: u32,
    pub(crate) column: u32,
}

#[derive(Debug)]
struct LocationData {
    source: Arc<str>,
    line: Option<u32>,
    column: Option<u32>,
    pointer: Option<String>,
}

/// The JSON Pointers of every value in a node tree, concatenated.
#[derive(Debug)]
pub(crate) struct LocationTable {
    source: Arc<str>,
    text: String,
    spans: Vec<Span>,
}

/// One table entry: the pointer's range in `text` plus its position (`0` when unknown).
#[derive(Debug)]
struct Span {
    start: u32,
    end: u32,
    line: u32,
    column: u32,
}

impl LocationTable {
    pub(crate) fn new(source: Arc<str>) -> Self {
        Self {
            source,
            text: String::new(),
            spans: Vec::new(),
        }
    }

    /// Records `pointer` at `position`, returning its index.
    pub(crate) fn push(&mut self, pointer: &str, position: Option<Position>) -> u32 {
        let start = self.text.len();
        self.text.push_str(pointer);
        let (line, column) = position.map_or((0, 0), |p| (p.line, p.column));
        self.spans.push(Span {
            start: index(start),
            end: index(self.text.len()),
            line,
            column,
        });
        index(self.spans.len() - 1)
    }

    /// The location of entry `index` of a shared table.
    pub(crate) fn location(table: &Arc<LocationTable>, index: u32) -> SourceLocation {
        SourceLocation(Repr::Table(table.clone(), index))
    }
}

fn index(value: usize) -> u32 {
    u32::try_from(value).expect("documents are limited far below 4 GiB")
}

impl SourceLocation {
    /// The location of a value that was not loaded from a source, such as a shape built in
    /// code.
    pub fn none() -> Self {
        Self(Repr::None)
    }

    fn data(data: LocationData) -> Self {
        Self(Repr::Data(Arc::new(data)))
    }

    pub(crate) fn new(source: Arc<str>) -> Self {
        Self::data(LocationData {
            source,
            line: None,
            column: None,
            pointer: None,
        })
    }

    pub(crate) fn with_pointer(
        source: &Arc<str>,
        pointer: String,
        position: Option<Position>,
    ) -> Self {
        Self::data(LocationData {
            source: source.clone(),
            line: position.map(|p| p.line),
            column: position.map(|p| p.column),
            pointer: Some(pointer),
        })
    }

    /// A parse-time position, completed later by the parser.
    pub(crate) fn pending(position: Position) -> Self {
        Self(Repr::Pending(position))
    }

    /// The line and column, including a pending parse-time position.
    pub(crate) fn position(&self) -> Option<Position> {
        match &self.0 {
            Repr::None => None,
            Repr::Pending(position) => Some(*position),
            _ => Some(Position {
                line: self.line()?,
                column: self.column()?,
            }),
        }
    }

    pub(crate) fn with_line_column(self, line: usize, column: usize) -> Self {
        let Some(source) = self.source_arc() else {
            return self;
        };
        Self::data(LocationData {
            source: source.clone(),
            line: u32::try_from(line).ok(),
            column: u32::try_from(column).ok(),
            pointer: self.pointer().map(str::to_owned),
        })
    }

    /// A location for `suffix` (an already-escaped pointer suffix such as `/members/a`)
    /// relative to this location, at this location's line and column. Unknown locations stay
    /// unknown.
    pub(crate) fn child(&self, suffix: &str) -> Self {
        self.child_at(suffix, None)
    }

    /// Like [`child`](Self::child), at `position` when known.
    ///
    /// When this location belongs to a table and the table records the child (every value of
    /// a loaded shape is recorded), the child's own entry and position are used.
    pub(crate) fn child_at(&self, suffix: &str, position: Option<Position>) -> Self {
        let Some(source) = self.source_arc() else {
            return Self::none();
        };
        let pointer = format!("{}{suffix}", self.pointer().unwrap_or(""));
        if let Repr::Table(table, at) = &self.0 {
            // Descendants follow their ancestor in preorder.
            let found = table.spans[*at as usize..]
                .iter()
                .position(|span| table.text[span.start as usize..span.end as usize] == pointer);
            if let Some(offset) = found {
                return Self(Repr::Table(table.clone(), *at + index(offset)));
            }
        }
        let position = position.or_else(|| self.position());
        Self::data(LocationData {
            source: source.clone(),
            line: position.map(|p| p.line),
            column: position.map(|p| p.column),
            pointer: Some(pointer),
        })
    }

    fn source_arc(&self) -> Option<&Arc<str>> {
        match &self.0 {
            Repr::None | Repr::Pending(_) => None,
            Repr::Data(data) => Some(&data.source),
            Repr::Table(table, _) => Some(&table.source),
        }
    }

    /// Returns `true` for [`SourceLocation::none`].
    pub fn is_none(&self) -> bool {
        matches!(self.0, Repr::None | Repr::Pending(_))
    }

    /// The caller-supplied source label, such as a file name; empty for
    /// [`SourceLocation::none`].
    pub fn source(&self) -> &str {
        self.source_arc().map_or("", |s| s)
    }

    /// The 1-based line, when known.
    pub fn line(&self) -> Option<u32> {
        match &self.0 {
            Repr::Data(data) => data.line,
            Repr::Table(table, index) => non_zero(table.spans[*index as usize].line),
            Repr::None | Repr::Pending(_) => None,
        }
    }

    /// The 1-based column, in characters, when known.
    pub fn column(&self) -> Option<u32> {
        match &self.0 {
            Repr::Data(data) => data.column,
            Repr::Table(table, index) => non_zero(table.spans[*index as usize].column),
            Repr::None | Repr::Pending(_) => None,
        }
    }

    /// The RFC 6901 JSON Pointer to the relevant value, when known.
    pub fn pointer(&self) -> Option<&str> {
        match &self.0 {
            Repr::None | Repr::Pending(_) => None,
            Repr::Data(data) => data.pointer.as_deref(),
            Repr::Table(table, index) => {
                let span = &table.spans[*index as usize];
                Some(&table.text[span.start as usize..span.end as usize])
            }
        }
    }

    /// Writes `source:line:column`, or just `source` when the position is unknown.
    fn write_position(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.source())?;
        if let (Some(line), Some(column)) = (self.line(), self.column()) {
            write!(f, ":{line}:{column}")?;
        }
        Ok(())
    }

    /// `source:line:column`, or the full location when the position is unknown.
    pub(crate) fn short(&self) -> impl fmt::Display + '_ {
        struct Short<'a>(&'a SourceLocation);
        impl fmt::Display for Short<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                if self.0.line().is_some() {
                    self.0.write_position(f)
                } else {
                    fmt::Display::fmt(self.0, f)
                }
            }
        }
        Short(self)
    }
}

/// Writes `message` the way [`Diagnostic`] does: `source:line:column: message (at pointer)`.
pub(crate) fn fmt_located(
    f: &mut fmt::Formatter<'_>,
    location: &SourceLocation,
    message: fmt::Arguments<'_>,
) -> fmt::Result {
    if !location.is_none() {
        location.write_position(f)?;
        f.write_str(": ")?;
    }
    f.write_fmt(message)?;
    match location.pointer() {
        Some(pointer) if !pointer.is_empty() => write!(f, " (at {pointer})"),
        _ => Ok(()),
    }
}

fn non_zero(value: u32) -> Option<u32> {
    (value != 0).then_some(value)
}

impl PartialEq for SourceLocation {
    fn eq(&self, other: &Self) -> bool {
        self.source_arc().map(|s| &**s) == other.source_arc().map(|s| &**s)
            && self.line() == other.line()
            && self.column() == other.column()
            && self.pointer() == other.pointer()
    }
}

impl Eq for SourceLocation {}

impl std::hash::Hash for SourceLocation {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.source_arc().map(|s| &**s).hash(state);
        self.line().hash(state);
        self.column().hash(state);
        self.pointer().hash(state);
    }
}

impl fmt::Debug for SourceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SourceLocation({self})")
    }
}

impl fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_none() {
            return f.write_str("<unknown>");
        }
        self.write_position(f)?;
        match self.pointer() {
            Some(pointer) if !pointer.is_empty() => write!(f, " ({pointer})"),
            _ => Ok(()),
        }
    }
}

/// Escapes one RFC 6901 reference token.
pub(crate) fn escape_pointer_token(token: &str) -> String {
    let mut out = String::with_capacity(token.len());
    push_pointer_token(&mut out, token);
    out
}

/// Appends one escaped RFC 6901 reference token to `pointer`, without allocating when the
/// buffer has capacity.
pub(crate) fn push_pointer_token(pointer: &mut String, token: &str) {
    if !token.contains(['~', '/']) {
        pointer.push_str(token);
        return;
    }
    for c in token.chars() {
        match c {
            '~' => pointer.push_str("~0"),
            '/' => pointer.push_str("~1"),
            c => pointer.push(c),
        }
    }
}

/// Structured feedback about a model.
///
/// Diagnostics are produced while loading, assembling, and validating a model. Custom
/// [validators](crate::validation::Validator) create them with [`Diagnostic::new`] or the
/// severity shortcuts:
///
/// ```
/// use aws_smithy_lang::diagnostic::{Diagnostic, DiagnosticCode, Severity};
///
/// let diagnostic = Diagnostic::warning(DiagnosticCode::Custom, "operation names should be verbs");
/// assert_eq!(diagnostic.severity(), Severity::Warning);
/// ```
///
/// `Display` puts the position first, like compiler output, so terminals and editors can link
/// to it. The JSON Pointer and any [related](Self::related) locations follow the message:
///
/// ```text
/// model.json:12:9: error[UnresolvedReference]: `ex#Missing` does not resolve to a shape (at /shapes/ex#L/member/target)
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    severity: Severity,
    code: DiagnosticCode,
    message: String,
    source: Option<SourceLocation>,
    related: Vec<SourceLocation>,
    shape_id: Option<ShapeId>,
    validator: Option<Arc<str>>,
}

impl Diagnostic {
    /// Creates a diagnostic with no location.
    pub fn new(severity: Severity, code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            severity,
            code,
            message: message.into(),
            source: None,
            related: Vec::new(),
            shape_id: None,
            validator: None,
        }
    }

    /// Creates an [`Error`](Severity::Error) diagnostic.
    pub fn error(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self::new(Severity::Error, code, message)
    }

    /// Creates a [`Warning`](Severity::Warning) diagnostic.
    pub fn warning(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, code, message)
    }

    /// Creates a [`Note`](Severity::Note) diagnostic.
    pub fn note(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self::new(Severity::Note, code, message)
    }

    pub(crate) fn error_at(
        code: DiagnosticCode,
        message: impl Into<String>,
        source: Option<SourceLocation>,
    ) -> Self {
        let diagnostic = Self::error(code, message);
        match source {
            Some(source) => diagnostic.with_source_location(source),
            None => diagnostic,
        }
    }

    /// Sets the primary location. [`SourceLocation::none`] clears it.
    pub fn with_source_location(mut self, location: SourceLocation) -> Self {
        self.source = (!location.is_none()).then_some(location);
        self
    }

    /// Adds another relevant location, such as the other side of a conflict.
    pub fn with_related(mut self, related: SourceLocation) -> Self {
        if !related.is_none() {
            self.related.push(related);
        }
        self
    }

    /// Sets the shape or member the diagnostic is about.
    pub fn with_shape_id(mut self, id: ShapeId) -> Self {
        self.shape_id = Some(id);
        self
    }

    pub(crate) fn with_validator(mut self, validator: &Arc<str>) -> Self {
        self.validator = Some(validator.clone());
        self
    }

    /// The severity.
    pub fn severity(&self) -> Severity {
        self.severity
    }

    /// The stable category.
    pub fn code(&self) -> DiagnosticCode {
        self.code
    }

    /// A human-readable message. Wording is not stable.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The primary location, when known.
    pub fn source_location(&self) -> Option<&SourceLocation> {
        self.source.as_ref()
    }

    /// Other relevant locations, such as the other side of a conflict.
    pub fn related(&self) -> &[SourceLocation] {
        &self.related
    }

    /// The shape or member the diagnostic is about, when known.
    pub fn shape_id(&self) -> Option<&ShapeId> {
        self.shape_id.as_ref()
    }

    /// The [`name`](crate::validation::Validator::name) of the validator that reported this
    /// diagnostic, or `None` for diagnostics from parsing and assembly.
    pub fn validator(&self) -> Option<&str> {
        self.validator.as_deref()
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let severity = match self.severity {
            Severity::Note => "note",
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        if let Some(source) = &self.source {
            source.write_position(f)?;
            f.write_str(": ")?;
        }
        write!(f, "{severity}[{:?}]", self.code)?;
        if let Some(validator) = &self.validator {
            write!(f, "[{validator}]")?;
        }
        write!(f, ": {}", self.message)?;
        let pointer = self
            .source
            .as_ref()
            .and_then(SourceLocation::pointer)
            .filter(|p| !p.is_empty());
        let mut details = pointer.map(|p| format!("at {p}")).into_iter().chain(
            self.related
                .iter()
                .map(|related| format!("see also {}", related.short())),
        );
        if let Some(first) = details.next() {
            write!(f, " ({first}")?;
            for detail in details {
                write!(f, "; {detail}")?;
            }
            f.write_str(")")?;
        }
        Ok(())
    }
}

/// An ordered collection of diagnostics.
///
/// Collections built while assembling are bounded by
/// [`ModelAssembler::max_diagnostics`](crate::ModelAssembler::max_diagnostics). Once the limit
/// is reached, one final [`DiagnosticCode::ResourceLimit`] error records that further
/// diagnostics were omitted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiagnosticSet {
    diagnostics: Vec<Diagnostic>,
    /// Maximum diagnostics recorded through `push`.
    limit: Option<usize>,
    /// Diagnostics recorded through `push`.
    bounded: usize,
    /// Number of diagnostics dropped because of `limit`.
    omitted: usize,
}

impl DiagnosticSet {
    pub(crate) fn with_limit(limit: usize) -> Self {
        Self {
            limit: Some(limit),
            ..Self::default()
        }
    }

    /// Records `diagnostic`, or drops it if the limit has been reached. The first dropped
    /// diagnostic is replaced with a truncation notice so the set still reports an error.
    pub(crate) fn push(&mut self, diagnostic: Diagnostic) {
        match self.limit {
            Some(limit) if self.bounded >= limit => {
                if self.omitted == 0 {
                    self.diagnostics.push(Diagnostic::error_at(
                        DiagnosticCode::ResourceLimit,
                        format!(
                            "diagnostic limit of {limit} reached; further diagnostics were omitted"
                        ),
                        diagnostic.source,
                    ));
                }
                self.omitted += 1;
            }
            _ => {
                self.bounded += 1;
                self.diagnostics.push(diagnostic);
            }
        }
    }

    /// Records `diagnostic` regardless of the limit. Used for the single fatal diagnostic
    /// that explains why parsing stopped.
    pub(crate) fn push_unbounded(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// Records every diagnostic of `other`, subject to this set's limit.
    pub(crate) fn extend(&mut self, other: DiagnosticSet) {
        for diagnostic in other.diagnostics {
            self.push(diagnostic);
        }
    }

    /// The number of retained diagnostics.
    pub fn len(&self) -> usize {
        self.diagnostics.len()
    }

    /// Returns `true` if there are no diagnostics.
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    /// Iterates all retained diagnostics in the order they were produced.
    pub fn iter(&self) -> std::slice::Iter<'_, Diagnostic> {
        self.diagnostics.iter()
    }

    /// Returns `true` if any diagnostic has [`Severity::Error`].
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    /// Iterates error diagnostics.
    pub fn errors(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
    }

    /// Returns `true` if any diagnostic has `code`.
    pub fn contains_code(&self, code: DiagnosticCode) -> bool {
        self.diagnostics.iter().any(|d| d.code == code)
    }

    /// Returns `true` if diagnostics were dropped because the configured limit was reached.
    pub fn is_truncated(&self) -> bool {
        self.omitted > 0
    }

    /// The number of diagnostics dropped because the configured limit was reached.
    pub fn omitted(&self) -> usize {
        self.omitted
    }
}

impl<'a> IntoIterator for &'a DiagnosticSet {
    type Item = &'a Diagnostic;
    type IntoIter = std::slice::Iter<'a, Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.diagnostics.iter()
    }
}

/// Loading a model failed.
///
/// Every collected diagnostic is available through [`LoadError::diagnostics`]. I/O and JSON
/// syntax failures also expose their underlying error through [`std::error::Error::source`].
#[derive(Debug)]
pub struct LoadError {
    kind: LoadErrorKind,
    diagnostics: DiagnosticSet,
}

#[derive(Debug)]
pub(crate) enum LoadErrorKind {
    Io { source: std::io::Error },
    Json { source: serde_json::Error },
    InvalidModel,
    ResourceLimit,
}

impl LoadError {
    pub(crate) fn io(source: std::io::Error, diagnostics: DiagnosticSet) -> Self {
        Self {
            kind: LoadErrorKind::Io { source },
            diagnostics,
        }
    }

    pub(crate) fn json(source: serde_json::Error, diagnostics: DiagnosticSet) -> Self {
        Self {
            kind: LoadErrorKind::Json { source },
            diagnostics,
        }
    }

    pub(crate) fn invalid_model(diagnostics: DiagnosticSet) -> Self {
        Self {
            kind: LoadErrorKind::InvalidModel,
            diagnostics,
        }
    }

    pub(crate) fn resource_limit(diagnostics: DiagnosticSet) -> Self {
        Self {
            kind: LoadErrorKind::ResourceLimit,
            diagnostics,
        }
    }

    pub(crate) fn from_kind(kind: LoadErrorKind, diagnostics: DiagnosticSet) -> Self {
        Self { kind, diagnostics }
    }

    pub(crate) fn into_parts(self) -> (LoadErrorKind, DiagnosticSet) {
        (self.kind, self.diagnostics)
    }

    /// All diagnostics collected before loading stopped.
    pub fn diagnostics(&self) -> &DiagnosticSet {
        &self.diagnostics
    }

    /// Consumes the error, returning its diagnostics.
    pub fn into_diagnostics(self) -> DiagnosticSet {
        self.diagnostics
    }

    /// Returns `true` if reading the input failed.
    pub fn is_io(&self) -> bool {
        matches!(self.kind, LoadErrorKind::Io { .. })
    }

    /// Returns `true` if the input was not valid JSON (including duplicate keys).
    pub fn is_json_syntax(&self) -> bool {
        matches!(self.kind, LoadErrorKind::Json { .. })
    }

    /// Returns `true` if the input was valid JSON but not a valid model.
    pub fn is_invalid_model(&self) -> bool {
        matches!(self.kind, LoadErrorKind::InvalidModel)
    }

    /// Returns `true` if a configured resource limit was exceeded.
    pub fn is_resource_limit(&self) -> bool {
        matches!(self.kind, LoadErrorKind::ResourceLimit)
    }
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let first = self.diagnostics.errors().next();
        let location = first.and_then(Diagnostic::source_location);
        match &self.kind {
            // The underlying error is the `source()`, so it is not repeated here.
            LoadErrorKind::Io { .. } => {
                f.write_str("failed to read Smithy model")?;
                if let Some(location) = location {
                    write!(f, " `{}`", location.source())?;
                }
                Ok(())
            }
            LoadErrorKind::Json { .. } => {
                f.write_str("failed to parse Smithy JSON AST")?;
                if let Some(location) = location {
                    write!(f, " at {}", location.short())?;
                }
                Ok(())
            }
            LoadErrorKind::ResourceLimit => {
                f.write_str("Smithy model exceeded a configured resource limit")?;
                write_first(f, location, first)
            }
            LoadErrorKind::InvalidModel => {
                f.write_str("invalid Smithy model")?;
                let errors = self.diagnostics.errors().count();
                if errors > 1 {
                    write!(f, " ({errors} errors); first")?;
                }
                write_first(f, location, first)
            }
        }
    }
}

/// Writes ` at <position>: <message>` for the first error.
fn write_first(
    f: &mut fmt::Formatter<'_>,
    location: Option<&SourceLocation>,
    first: Option<&Diagnostic>,
) -> fmt::Result {
    if let Some(location) = location {
        write!(f, " at {}", location.short())?;
    }
    if let Some(first) = first {
        write!(f, ": {}", first.message())?;
    }
    Ok(())
}

impl std::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            LoadErrorKind::Io { source } => Some(source),
            LoadErrorKind::Json { source } => Some(source),
            LoadErrorKind::InvalidModel | LoadErrorKind::ResourceLimit => None,
        }
    }
}

/// Writing a model failed.
#[derive(Debug)]
pub struct WriteError {
    kind: WriteErrorKind,
}

#[derive(Debug)]
enum WriteErrorKind {
    Io { source: std::io::Error },
    Json { source: serde_json::Error },
}

impl WriteError {
    pub(crate) fn from_json(source: serde_json::Error) -> Self {
        let kind = if source.is_io() {
            WriteErrorKind::Io {
                source: source.into(),
            }
        } else {
            WriteErrorKind::Json { source }
        };
        Self { kind }
    }

    /// Returns `true` if the underlying writer failed.
    pub fn is_io(&self) -> bool {
        matches!(self.kind, WriteErrorKind::Io { .. })
    }
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("failed to write Smithy JSON AST")
    }
}

impl std::error::Error for WriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            WriteErrorKind::Io { source } => Some(source),
            WriteErrorKind::Json { source } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;

    #[test]
    fn errors_are_send_sync_static() {
        fn assert<T: std::error::Error + Send + Sync + 'static>() {}
        assert::<LoadError>();
        assert::<WriteError>();
        fn assert_data<T: Send + Sync + 'static>() {}
        assert_data::<Diagnostic>();
        assert_data::<DiagnosticSet>();
    }

    #[test]
    fn source_chain() {
        let io = std::io::Error::other("boom");
        let err = LoadError::io(io, DiagnosticSet::default());
        assert!(err.is_io());
        assert_eq!(err.source().unwrap().to_string(), "boom");

        let mut diagnostics = DiagnosticSet::default();
        diagnostics.push(Diagnostic::error_at(
            DiagnosticCode::InvalidTarget,
            "bad target",
            Some(SourceLocation::with_pointer(
                &Arc::from("m.json"),
                "/shapes/a#B".into(),
                Some(Position { line: 3, column: 5 }),
            )),
        ));
        let err = LoadError::invalid_model(diagnostics);
        assert!(err.source().is_none());
        assert_eq!(
            err.to_string(),
            "invalid Smithy model at m.json:3:5: bad target"
        );
        let d = err.diagnostics().iter().next().unwrap();
        assert_eq!(
            d.to_string(),
            "m.json:3:5: error[InvalidTarget]: bad target (at /shapes/a#B)"
        );
    }

    #[test]
    fn pointer_escaping() {
        assert_eq!(escape_pointer_token("a/b~c"), "a~1b~0c");
    }
}
