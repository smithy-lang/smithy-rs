/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Structured model diagnostics and operation errors.

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
}

/// A location within a model source.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceLocation {
    source: Arc<str>,
    line: Option<u32>,
    column: Option<u32>,
    pointer: Option<String>,
}

impl SourceLocation {
    pub(crate) fn new(source: Arc<str>) -> Self {
        Self {
            source,
            line: None,
            column: None,
            pointer: None,
        }
    }

    pub(crate) fn with_pointer(source: &Arc<str>, pointer: String) -> Self {
        Self {
            source: source.clone(),
            line: None,
            column: None,
            pointer: Some(pointer),
        }
    }

    pub(crate) fn with_line_column(mut self, line: usize, column: usize) -> Self {
        self.line = u32::try_from(line).ok();
        self.column = u32::try_from(column).ok();
        self
    }

    /// A location for `suffix` (an already-escaped pointer suffix such as `/members/a`)
    /// relative to this location.
    pub(crate) fn child(&self, suffix: &str) -> Self {
        let mut child = self.clone();
        child.pointer = Some(format!("{}{suffix}", self.pointer.as_deref().unwrap_or("")));
        child
    }

    /// The caller-supplied source label, such as a file name.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The 1-based line, when known.
    pub fn line(&self) -> Option<u32> {
        self.line
    }

    /// The 1-based column, when known.
    pub fn column(&self) -> Option<u32> {
        self.column
    }

    /// The RFC 6901 JSON Pointer to the relevant value, when known.
    pub fn pointer(&self) -> Option<&str> {
        self.pointer.as_deref()
    }
}

impl fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.source)?;
        if let (Some(line), Some(column)) = (self.line, self.column) {
            write!(f, ":{line}:{column}")?;
        }
        if let Some(pointer) = &self.pointer {
            write!(f, "#{pointer}")?;
        }
        Ok(())
    }
}

/// Escapes one RFC 6901 reference token.
pub(crate) fn escape_pointer_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

/// Structured feedback about a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    severity: Severity,
    code: DiagnosticCode,
    message: String,
    source: Option<SourceLocation>,
    related: Vec<SourceLocation>,
}

impl Diagnostic {
    pub(crate) fn error(
        code: DiagnosticCode,
        message: impl Into<String>,
        source: Option<SourceLocation>,
    ) -> Self {
        Self {
            severity: Severity::Error,
            code,
            message: message.into(),
            source,
            related: Vec::new(),
        }
    }

    pub(crate) fn with_related(mut self, related: SourceLocation) -> Self {
        self.related.push(related);
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
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let severity = match self.severity {
            Severity::Note => "note",
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        write!(f, "{severity}[{:?}]: {}", self.code, self.message)?;
        if let Some(source) = &self.source {
            write!(f, " (at {source})")?;
        }
        Ok(())
    }
}

/// An ordered collection of diagnostics.
///
/// Collections built while loading are bounded by
/// [`ModelLoader::max_diagnostics`](crate::ModelLoader::max_diagnostics). Once the limit is
/// reached, one final [`DiagnosticCode::ResourceLimit`] error records that further diagnostics
/// were omitted.
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
                    self.diagnostics.push(Diagnostic::error(
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
enum LoadErrorKind {
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
        let errors = self.diagnostics.errors().count();
        match &self.kind {
            LoadErrorKind::Io { .. } => f.write_str("failed to read Smithy model"),
            LoadErrorKind::Json { .. } => f.write_str("failed to parse Smithy JSON AST"),
            LoadErrorKind::ResourceLimit => {
                f.write_str("Smithy model exceeded a configured resource limit")
            }
            LoadErrorKind::InvalidModel => {
                write!(f, "invalid Smithy model ({errors} error")?;
                if errors != 1 {
                    f.write_str("s")?;
                }
                f.write_str(")")?;
                if let Some(first) = self.diagnostics.errors().next() {
                    write!(f, ": {}", first.message())?;
                }
                Ok(())
            }
        }
    }
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
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::InvalidTarget,
            "bad target",
            Some(SourceLocation::with_pointer(
                &Arc::from("m.json"),
                "/shapes/a#B".into(),
            )),
        ));
        let err = LoadError::invalid_model(diagnostics);
        assert!(err.source().is_none());
        assert_eq!(
            err.to_string(),
            "invalid Smithy model (1 error): bad target"
        );
        let d = err.diagnostics().iter().next().unwrap();
        assert_eq!(
            d.to_string(),
            "error[InvalidTarget]: bad target (at m.json#/shapes/a#B)"
        );
    }

    #[test]
    fn pointer_escaping() {
        assert_eq!(escape_pointer_token("a/b~c"), "a~1b~0c");
    }
}
