/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Schema-aware wrapper around [`Document`] or [`DocumentV2`].
//!
//! [`DiscriminatedDocument`] adds two pieces of context that the bare
//! `Document` data type deliberately doesn't carry:
//!
//! - An optional **discriminator** — the Smithy fully-qualified shape
//!   ID of the type the document was produced from (or is intended to
//!   deserialize as). This drives the type-registry path: a JSON
//!   `__type` field gets lifted into the discriminator slot during
//!   wire parsing, and the type registry uses it to dispatch to the
//!   right schema.
//!
//! - Optional **protocol settings** — a
//!   [`DocumentSettings`] trait object
//!   describing how the source protocol encodes Smithy types that
//!   don't have native wire representations. Used by the format-aware
//!   accessors [`as_blob`](DiscriminatedDocument::as_blob) and
//!   [`as_timestamp`](DiscriminatedDocument::as_timestamp) to coerce
//!   JSON-side base64 strings into bytes, ISO-8601 strings into
//!   timestamps, etc.
//!
//! Why split this from `Document`? Two reasons:
//!
//! 1. The bare `Document` is the everyday user-facing type — it
//!    appears on operation input/output struct fields,
//!    `AuthSchemeEndpointConfig::as_document`, and in user code
//!    constructing values. Pattern matching, builders, and round-trip
//!    semantics on it should stay simple. A user holding an
//!    `Option<Document>` doesn't need to think about
//!    discriminators.
//! 2. The schema-serde pipeline does need both pieces of context, and
//!    the cleanest place for it is right here on the wrapper. The
//!    type registry's `deserialize_document` flow consumes
//!    `&DiscriminatedDocument`; codec deserializers produce
//!    `DiscriminatedDocument` (with discriminator lifted from
//!    `__type` and settings attached); type-typed shape construction
//!    via `DiscriminatedDocumentExt::from_struct` (in
//!    `aws-smithy-schema`) returns a `DiscriminatedDocument` too.
//!
//! The wrapper is generic over the document type it holds, which is
//! either [`Document`] or [`DocumentV2`]. The type parameter defaults to
//! `Document`, so `DiscriminatedDocument` names
//! `DiscriminatedDocument<Document>`, as it did before the parameter
//! existed.

use std::borrow::Cow;
use std::sync::Arc;

use crate::date_time::Format;
use crate::document::document_variant_name;
use crate::{DateTime, Document, DocumentError, DocumentSettings, DocumentV2, Number};

mod v2;

/// A document together with an optional discriminator and
/// optional protocol settings.
///
/// See the module-level documentation for the rationale behind
/// splitting this off `Document`.
///
/// `D` is the wrapped document type: [`Document`], the default, or
/// [`DocumentV2`]. Most methods are available for both.
/// [`new`](DiscriminatedDocument::new) and
/// [`with_discriminator`](DiscriminatedDocument::with_discriminator) exist
/// only for `DiscriminatedDocument<Document>`, and
/// [`new_v2`](DiscriminatedDocument::new_v2) only for
/// `DiscriminatedDocument<DocumentV2>`.
///
/// This type is `#[non_exhaustive]` so that future fields (e.g. a typed
/// `Schema` reference, if the schema crate ever adds schema-binding to
/// this wrapper) can land additively.
///
/// Note that this is deliberately *unlike* [`Document`], which is
/// intentionally **not** `#[non_exhaustive]`: `Document` is a released,
/// exhaustively-matchable enum, and callers are expected to `match` it
/// without a wildcard arm. Adding a variant to `Document` would be a
/// breaking change, which is precisely why the extra context this
/// wrapper carries lives here instead of on `Document` itself.
#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct DiscriminatedDocument<D = Document> {
    /// The wrapped document data. Always present.
    document: D,
    /// The fully-qualified shape ID of the source type, if known.
    /// Lifted from `__type` on the wire, set by
    /// `DiscriminatedDocumentExt::from_struct` (in `aws-smithy-schema`)
    /// callers, or left `None` for documents constructed directly from
    /// data.
    discriminator: Option<String>,
    /// Protocol-specific settings used by format-aware coercion. Set
    /// by codec deserializers (e.g. JSON's
    /// `read_discriminated_document`), left `None` on user-built
    /// documents.
    settings: Option<Arc<dyn DocumentSettings>>,
}

/// A document type that [`DiscriminatedDocument`] can wrap: [`Document`] or [`DocumentV2`].
///
/// Use it as a bound to write code that accepts either wrapper:
///
/// ```
/// use aws_smithy_types::document::{DiscriminatedDocument, DocumentTrait};
/// use aws_smithy_types::{Document, DocumentV2};
///
/// fn discriminator<D: DocumentTrait>(document: &DiscriminatedDocument<D>) -> Option<&str> {
///     document.discriminator()
/// }
///
/// let v1 = DiscriminatedDocument::new(Document::Null);
/// let v2 = DiscriminatedDocument::new_v2(DocumentV2::Null);
/// assert_eq!(discriminator(&v1), discriminator(&v2));
/// ```
///
/// The trait is sealed, so only this crate can implement it:
///
/// ```compile_fail,E0277
/// use aws_smithy_types::document::DocumentTrait;
///
/// struct MyDocument;
/// impl DocumentTrait for MyDocument {}
/// ```
pub trait DocumentTrait: sealed::Sealed {}

impl DocumentTrait for Document {}
impl DocumentTrait for DocumentV2 {}

mod sealed {
    use super::*;

    /// The per-document-type behavior of [`DiscriminatedDocument`].
    ///
    /// The trait is public so that it can be a supertrait of [`DocumentTrait`], and this module
    /// is private so that no other crate can implement it.
    pub trait Sealed {
        /// Implements [`DiscriminatedDocument::as_blob`].
        fn coerce_to_blob<'a>(
            &'a self,
            settings: Option<&dyn DocumentSettings>,
        ) -> Result<Cow<'a, [u8]>, DocumentError>;

        /// Implements [`DiscriminatedDocument::as_timestamp`].
        fn coerce_to_timestamp(
            &self,
            settings: Option<&dyn DocumentSettings>,
        ) -> Result<DateTime, DocumentError>;
    }
}

impl<D: DocumentTrait> DiscriminatedDocument<D> {
    /// Wraps `document`, with no discriminator and no settings.
    fn wrap(document: D) -> Self {
        Self {
            document,
            discriminator: None,
            settings: None,
        }
    }

    /// Attaches a discriminator: the absolute shape ID of the shape this document represents.
    ///
    /// The SEP requires a serialized discriminator to be absolute so that the document can be
    /// read without other context, and the type registry looks shapes up by absolute ID. This
    /// returns [`DocumentError::InvalidInput`] unless `shape_id` has the form
    /// `namespace#Name`, where the namespace is one or more dot-separated Smithy identifiers
    /// and the name is a Smithy identifier. A member ID such as `namespace#Name$member` is
    /// rejected.
    ///
    /// Unlike [`with_discriminator`](DiscriminatedDocument::with_discriminator), this checks
    /// the shape ID in every build.
    pub fn try_with_discriminator(
        mut self,
        shape_id: impl Into<String>,
    ) -> Result<Self, DocumentError> {
        let shape_id = shape_id.into();
        if !is_absolute_shape_id(&shape_id) {
            return Err(DocumentError::invalid_input(format!(
                "discriminator {shape_id:?} must be an absolute shape ID (namespace#Name)"
            )));
        }
        self.discriminator = Some(shape_id);
        Ok(self)
    }

    /// Attaches protocol settings to this document.
    ///
    /// Used by codec deserializers to plumb format-specific coercion
    /// rules through to downstream consumers of the document tree.
    /// The same `Arc` is cloned into nested documents so the entire
    /// tree shares one settings instance.
    pub fn with_settings(mut self, settings: Arc<dyn DocumentSettings>) -> Self {
        self.settings = Some(settings);
        self
    }

    /// Sets the discriminator **without** the absolute-shape-id check
    /// that [`with_discriminator`](DiscriminatedDocument::with_discriminator) enforces.
    ///
    /// Test-only escape hatch (behind the `test-util` feature) so that
    /// downstream crates can construct a document carrying a relative
    /// (non-absolute) discriminator to exercise their own guards
    /// against one reaching the wire. Production code MUST use
    /// [`with_discriminator`](DiscriminatedDocument::with_discriminator)
    /// or [`try_with_discriminator`](Self::try_with_discriminator), which
    /// guarantee the absolute `namespace#name` form the SEP requires.
    #[cfg(feature = "test-util")]
    #[doc(hidden)]
    pub fn set_discriminator_unchecked(&mut self, discriminator: impl Into<String>) {
        self.discriminator = Some(discriminator.into());
    }

    /// Returns the discriminator, if attached.
    pub fn discriminator(&self) -> Option<&str> {
        self.discriminator.as_deref()
    }

    /// Returns a reference to the attached protocol settings, if any.
    pub fn settings(&self) -> Option<&Arc<dyn DocumentSettings>> {
        self.settings.as_ref()
    }

    /// Returns a reference to the wrapped document data.
    pub fn document(&self) -> &D {
        &self.document
    }

    /// Consumes this wrapper and returns the inner document.
    pub fn into_document(self) -> D {
        self.document
    }

    /// Returns this document's value as bytes.
    ///
    /// - A [`DocumentV2::Blob`] is returned as is, whether or not settings
    ///   are attached.
    /// - A string is decoded with
    ///   [`DocumentSettings::coerce_string_to_blob`] when settings are
    ///   attached, and with the deterministic default, a standard base64
    ///   decode, otherwise.
    ///
    /// [`Document`] has no native blob variant: the schema-driven legacy
    /// representation of a Smithy `blob` is a base64-encoded
    /// [`Document::String`]. For a `Document`, only the string case
    /// applies.
    ///
    /// Other variants return [`DocumentError::TypeMismatch`].
    pub fn as_blob(&self) -> Result<Cow<'_, [u8]>, DocumentError> {
        self.document.coerce_to_blob(self.settings.as_deref())
    }

    /// Returns this document's value as a timestamp.
    ///
    /// - A [`DocumentV2::Timestamp`] is returned as is, whether or not
    ///   settings are attached.
    /// - A number is converted with
    ///   [`DocumentSettings::coerce_number_to_timestamp`] when settings are
    ///   attached, and read as epoch seconds, the deterministic default,
    ///   otherwise.
    /// - A string is converted with
    ///   [`DocumentSettings::coerce_string_to_timestamp`] when settings are
    ///   attached, and parsed as RFC-3339 (`date-time`), which is Smithy's
    ///   default string timestamp format, otherwise.
    ///
    /// [`Document`] has no native timestamp variant: the schema-driven
    /// legacy representation of a Smithy `timestamp` is a
    /// [`Document::Number`] holding epoch seconds. For a `Document`, only
    /// the number and string cases apply.
    ///
    /// Other variants return [`DocumentError::TypeMismatch`].
    pub fn as_timestamp(&self) -> Result<DateTime, DocumentError> {
        self.document.coerce_to_timestamp(self.settings.as_deref())
    }
}

impl DiscriminatedDocument<Document> {
    /// Creates a new `DiscriminatedDocument` wrapping `document`,
    /// with no discriminator and no settings attached.
    ///
    /// Use [`new_v2`](DiscriminatedDocument::new_v2) to wrap a
    /// [`DocumentV2`].
    pub fn new(document: Document) -> Self {
        Self::wrap(document)
    }

    /// Attaches a discriminator (a Smithy fully-qualified shape ID)
    /// to this document.
    ///
    /// Used by the schema-serde pipeline when constructing a
    /// document from a typed shape: the schema's `shape_id` (in its
    /// `namespace#name` FQN form) gets attached as the discriminator
    /// so downstream consumers (the type registry, the `__type`
    /// write path) know what shape the document represents.
    ///
    /// The discriminator MUST be an absolute shape ID — the
    /// `namespace#name` FQN form, never a bare shape name. The SEP
    /// requires a serialized `__type` to always be absolute so the
    /// document stays context-free for downstream readers, and the
    /// type registry keys on the FQN. Passing a relative id is a
    /// caller error and trips a `debug_assert!`.
    /// [`try_with_discriminator`](DiscriminatedDocument::try_with_discriminator)
    /// checks the full shape ID grammar in every build instead.
    pub fn with_discriminator(mut self, fqn: impl Into<String>) -> Self {
        let fqn = fqn.into();
        debug_assert!(
            fqn.contains('#'),
            "discriminator `{fqn}` must be an absolute shape id (namespace#name)"
        );
        self.discriminator = Some(fqn);
        self
    }
}

impl sealed::Sealed for Document {
    fn coerce_to_blob<'a>(
        &'a self,
        settings: Option<&dyn DocumentSettings>,
    ) -> Result<Cow<'a, [u8]>, DocumentError> {
        match self {
            Document::String(s) => string_as_blob(s, settings),
            other => Err(DocumentError::type_mismatch(format!(
                "expected blob, found {}",
                document_variant_name(other)
            ))),
        }
    }

    fn coerce_to_timestamp(
        &self,
        settings: Option<&dyn DocumentSettings>,
    ) -> Result<DateTime, DocumentError> {
        match self {
            Document::Number(n) => number_as_timestamp(n, settings),
            Document::String(s) => string_as_timestamp(s, settings),
            other => Err(DocumentError::type_mismatch(format!(
                "expected timestamp, found {}",
                document_variant_name(other)
            ))),
        }
    }
}

/// `PartialEq` is implemented manually to compare `document` and
/// `discriminator` only — `settings` is metadata about how the
/// document was produced (and `dyn DocumentSettings` doesn't admit
/// equality anyway).
///
/// Two discriminated documents holding the same data with different
/// settings are considered equal: this matches the behavior of the
/// schema-crate type that this design replaces, and matches user
/// intent ("are these the same document?" doesn't depend on protocol
/// metadata).
impl<D: PartialEq> PartialEq for DiscriminatedDocument<D> {
    fn eq(&self, other: &Self) -> bool {
        self.document == other.document && self.discriminator == other.discriminator
    }
}

/// The default value wraps `Document::Null`, with no discriminator and no settings.
///
/// This is implemented only for `DiscriminatedDocument<Document>`; see the compatibility
/// rules at the top of this module.
impl Default for DiscriminatedDocument<Document> {
    fn default() -> Self {
        Self::new(Document::default())
    }
}

impl From<Document> for DiscriminatedDocument<Document> {
    fn from(document: Document) -> Self {
        Self::new(document)
    }
}

/// Decodes a string as a blob: with `settings` when attached, and as standard base64
/// otherwise.
fn string_as_blob<'a>(
    s: &str,
    settings: Option<&dyn DocumentSettings>,
) -> Result<Cow<'a, [u8]>, DocumentError> {
    match settings {
        Some(settings) => settings.coerce_string_to_blob(s).map(Cow::Owned),
        // Deterministic default: base64, the representation every
        // JSON-family protocol uses and the one the schema
        // serializer writes.
        None => crate::base64::decode(s).map(Cow::Owned).map_err(|e| {
            DocumentError::invalid_input(format!("cannot base64-decode string as blob: {e}"))
        }),
    }
}

/// Converts a number to a timestamp: with `settings` when attached, and as epoch seconds
/// otherwise.
fn number_as_timestamp(
    n: &Number,
    settings: Option<&dyn DocumentSettings>,
) -> Result<DateTime, DocumentError> {
    match settings {
        Some(settings) => settings.coerce_number_to_timestamp(n),
        None => number_as_epoch_seconds(n),
    }
}

/// Converts a string to a timestamp: with `settings` when attached, and as an RFC-3339
/// `date-time` otherwise.
fn string_as_timestamp(
    s: &str,
    settings: Option<&dyn DocumentSettings>,
) -> Result<DateTime, DocumentError> {
    match settings {
        Some(settings) => settings.coerce_string_to_timestamp(s),
        None => DateTime::from_str(s, Format::DateTime).map_err(|e| {
            DocumentError::invalid_input(format!(
                "cannot parse string as a date-time timestamp: {e}"
            ))
        }),
    }
}

/// Interprets a [`Number`] as epoch seconds, retaining fractional
/// seconds when the wire value carried them.
///
/// This is the deterministic default used when no protocol settings are
/// attached. `epoch-seconds` is the format the schema serializer writes
/// for a `timestamp` shape in the legacy `Document` representation.
fn number_as_epoch_seconds(n: &Number) -> Result<DateTime, DocumentError> {
    match n {
        Number::PosInt(v) => i64::try_from(*v)
            .map(DateTime::from_secs)
            .map_err(|_| DocumentError::invalid_input(format!("epoch seconds {v} out of range"))),
        Number::NegInt(v) => Ok(DateTime::from_secs(*v)),
        Number::Float(v) => {
            if !v.is_finite() {
                return Err(DocumentError::invalid_input(format!(
                    "epoch seconds {v} is not finite"
                )));
            }
            Ok(DateTime::from_secs_f64(*v))
        }
    }
}

/// Whether `shape_id` is an absolute Smithy shape ID without a member: `namespace#Name`.
fn is_absolute_shape_id(shape_id: &str) -> bool {
    match shape_id.split_once('#') {
        Some((namespace, name)) => namespace.split('.').all(is_identifier) && is_identifier(name),
        None => false,
    }
}

/// Whether `s` matches the Smithy `identifier` grammar:
/// `(1*"_" (ALPHA / DIGIT) / ALPHA) *(ALPHA / DIGIT / "_")`.
fn is_identifier(s: &str) -> bool {
    let rest = s.trim_start_matches('_');
    let starts_correctly = match rest.bytes().next() {
        // After leading underscores, a letter or digit must follow.
        Some(first) if rest.len() < s.len() => first.is_ascii_alphanumeric(),
        Some(first) => first.is_ascii_alphabetic(),
        None => false,
    };
    starts_correctly && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

#[cfg(test)]
mod tests {
    //! Tests for the `DiscriminatedDocument` wrapper and the
    //! `DocumentSettings` dispatch path.
    //!
    //! `TestSettings` is a minimal mock implementation of
    //! `DocumentSettings` — it doesn't try to do anything realistic
    //! (the JSON codec's settings will base64-decode, parse RFC-3339
    //! timestamps, etc.). The point is to exercise dispatch and
    //! error-path coverage; realistic implementations live in the
    //! codec crates that pair with this type.

    use super::*;
    use crate::Number;

    #[derive(Debug)]
    struct TestSettings {
        protocol: String,
    }

    impl DocumentSettings for TestSettings {
        fn protocol_id(&self) -> &str {
            &self.protocol
        }

        fn coerce_string_to_blob(&self, s: &str) -> Result<Vec<u8>, DocumentError> {
            // Mock: just return the bytes of the string itself.
            // A real impl would base64-decode for JSON.
            Ok(s.as_bytes().to_vec())
        }

        fn coerce_string_to_timestamp(&self, _s: &str) -> Result<DateTime, DocumentError> {
            // Mock: always return epoch.
            Ok(DateTime::from_secs(0))
        }

        fn coerce_number_to_timestamp(&self, n: &Number) -> Result<DateTime, DocumentError> {
            let secs = match n {
                Number::PosInt(v) => *v as i64,
                Number::NegInt(v) => *v,
                Number::Float(v) => *v as i64,
            };
            Ok(DateTime::from_secs(secs))
        }
    }

    fn test_settings() -> Arc<dyn DocumentSettings> {
        Arc::new(TestSettings {
            protocol: "com.example#Test".to_owned(),
        })
    }

    // -- Constructors and accessors -------------------------------------

    #[test]
    fn new_attaches_no_discriminator_or_settings() {
        let d = DiscriminatedDocument::new(Document::String("hi".to_owned()));
        assert_eq!(d.discriminator(), None);
        assert!(d.settings().is_none());
        assert_eq!(d.document().as_string(), Some("hi"));
    }

    #[test]
    fn with_discriminator_attaches_fqn() {
        let d =
            DiscriminatedDocument::new(Document::Null).with_discriminator("com.example#MyShape");
        assert_eq!(d.discriminator(), Some("com.example#MyShape"));
    }

    // The discriminator must be an absolute shape id; a bare name is a
    // caller error and trips the debug-build assertion.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "absolute shape id")]
    fn with_discriminator_rejects_relative_id() {
        let _ = DiscriminatedDocument::new(Document::Null).with_discriminator("RelativeOnly");
    }

    #[test]
    fn with_settings_attaches_settings() {
        let d = DiscriminatedDocument::new(Document::String("x".to_owned()))
            .with_settings(test_settings());
        assert!(d.settings().is_some());
        assert_eq!(d.settings().unwrap().protocol_id(), "com.example#Test");
    }

    #[test]
    fn into_document_unwraps_to_inner() {
        let inner = Document::String("hi".to_owned());
        let d = DiscriminatedDocument::new(inner.clone()).with_discriminator("com.example#X");
        assert_eq!(d.into_document(), inner);
    }

    #[test]
    fn from_document_blanket_impl_works() {
        let d: DiscriminatedDocument = Document::Bool(true).into();
        assert_eq!(d.document().as_bool(), Some(true));
        assert_eq!(d.discriminator(), None);
    }

    #[test]
    fn default_wraps_null() {
        let d = DiscriminatedDocument::default();
        assert_eq!(d.document(), &Document::Null);
        assert_eq!(d.discriminator(), None);
        assert!(d.settings().is_none());
    }

    // -- Shape ID validation --------------------------------------------

    #[test]
    fn try_with_discriminator_accepts_absolute_shape_ids() {
        for shape_id in [
            "com.example#MyShape",
            "example#Shape",
            "smithy.api#String",
            "com._internal#_1Shape",
            "a.b_c.D9#E_f",
        ] {
            let d = DiscriminatedDocument::new(Document::Null)
                .try_with_discriminator(shape_id)
                .unwrap_or_else(|e| panic!("{shape_id}: {e}"));
            assert_eq!(d.discriminator(), Some(shape_id));
        }
    }

    #[test]
    fn try_with_discriminator_rejects_other_strings() {
        for shape_id in [
            "",
            "MyShape",
            "#MyShape",
            "com.example#",
            "com.example#My#Shape",
            "com.example#MyShape$member",
            "com..example#MyShape",
            ".com#MyShape",
            "com.example.#MyShape",
            "com.example#9Shape",
            "com.example#_",
            "1com#MyShape",
            "com example#MyShape",
            "com.example#My-Shape",
        ] {
            let result =
                DiscriminatedDocument::new(Document::Null).try_with_discriminator(shape_id);
            assert!(
                matches!(result, Err(DocumentError::InvalidInput { .. })),
                "{shape_id:?} should be rejected"
            );
        }
    }

    // -- Equality ignores settings --------------------------------------

    #[test]
    fn partial_eq_compares_document_and_discriminator_only() {
        let a = DiscriminatedDocument::new(Document::String("x".to_owned()))
            .with_discriminator("com.example#A");
        let b = DiscriminatedDocument::new(Document::String("x".to_owned()))
            .with_discriminator("com.example#A")
            .with_settings(test_settings());
        // Different settings (none vs Some), but same data + same
        // discriminator: equal.
        assert_eq!(a, b);

        let c = DiscriminatedDocument::new(Document::String("x".to_owned()))
            .with_discriminator("com.example#B");
        // Different discriminator: NOT equal.
        assert_ne!(a, c);

        let d = DiscriminatedDocument::new(Document::String("y".to_owned()))
            .with_discriminator("com.example#A");
        // Different data: NOT equal.
        assert_ne!(a, d);
    }

    // -- as_blob dispatch -----------------------------------------------

    #[test]
    fn as_blob_coerces_string_when_settings_present() {
        let d = DiscriminatedDocument::new(Document::String("hello".to_owned()))
            .with_settings(test_settings());
        match d.as_blob().unwrap() {
            // TestSettings returns the raw bytes of the string.
            Cow::Owned(bytes) => assert_eq!(bytes, b"hello"),
            Cow::Borrowed(_) => panic!("expected Cow::Owned for coerced String"),
        }
    }

    #[test]
    fn as_blob_falls_back_to_base64_without_settings() {
        // No settings attached: the deterministic default is a standard
        // base64 decode, matching what the schema serializer writes.
        let d = DiscriminatedDocument::new(Document::String("YWJjZA==".to_owned()));
        assert!(d.settings().is_none());
        assert_eq!(d.as_blob().unwrap().as_ref(), b"abcd");
    }

    #[test]
    fn as_blob_invalid_base64_without_settings_is_invalid_input() {
        let d = DiscriminatedDocument::new(Document::String("not base64!!!".to_owned()));
        let err = d.as_blob().unwrap_err();
        assert!(matches!(err, DocumentError::InvalidInput { .. }));
    }

    #[test]
    fn as_blob_type_mismatch_for_non_string_variants() {
        // Numeric variant: TypeMismatch regardless of settings.
        let d = DiscriminatedDocument::new(Document::Number(Number::PosInt(42)))
            .with_settings(test_settings());
        let err = d.as_blob().unwrap_err();
        assert!(matches!(err, DocumentError::TypeMismatch { .. }));
    }

    // -- as_timestamp dispatch ------------------------------------------

    #[test]
    fn as_timestamp_coerces_string_with_settings() {
        let d = DiscriminatedDocument::new(Document::String("any string".to_owned()))
            .with_settings(test_settings());
        // Mock returns epoch.
        assert_eq!(d.as_timestamp().unwrap(), DateTime::from_secs(0));
    }

    #[test]
    fn as_timestamp_coerces_number_with_settings() {
        let d = DiscriminatedDocument::new(Document::Number(Number::PosInt(1234)))
            .with_settings(test_settings());
        assert_eq!(d.as_timestamp().unwrap(), DateTime::from_secs(1234));
    }

    #[test]
    fn as_timestamp_number_defaults_to_epoch_seconds_without_settings() {
        // The deterministic default: a number is epoch seconds. This is
        // the legacy representation the schema serializer writes for a
        // `timestamp` shape.
        let d = DiscriminatedDocument::new(Document::Number(Number::PosInt(1234)));
        assert!(d.settings().is_none());
        assert_eq!(d.as_timestamp().unwrap(), DateTime::from_secs(1234));

        let d = DiscriminatedDocument::new(Document::Number(Number::NegInt(-5)));
        assert_eq!(d.as_timestamp().unwrap(), DateTime::from_secs(-5));
    }

    #[test]
    fn as_timestamp_retains_fractional_epoch_seconds() {
        let d = DiscriminatedDocument::new(Document::Number(Number::Float(1234.5)));
        assert_eq!(d.as_timestamp().unwrap(), DateTime::from_secs_f64(1234.5));
    }

    #[test]
    fn as_timestamp_string_defaults_to_date_time_without_settings() {
        let d = DiscriminatedDocument::new(Document::String("1970-01-01T00:00:00Z".to_owned()));
        assert_eq!(d.as_timestamp().unwrap(), DateTime::from_secs(0));
    }

    #[test]
    fn as_timestamp_malformed_string_without_settings_is_invalid_input() {
        let d = DiscriminatedDocument::new(Document::String("not a timestamp".to_owned()));
        let err = d.as_timestamp().unwrap_err();
        assert!(matches!(err, DocumentError::InvalidInput { .. }));
    }

    #[test]
    fn as_timestamp_type_mismatch_for_non_coercible_variant() {
        // Boolean can never coerce to timestamp regardless of settings.
        let d = DiscriminatedDocument::new(Document::Bool(true)).with_settings(test_settings());
        let err = d.as_timestamp().unwrap_err();
        assert!(matches!(err, DocumentError::TypeMismatch { .. }));
    }

    // -- Default trait method bodies emit UnsupportedOperation ----------

    #[test]
    fn default_settings_methods_return_unsupported_operation() {
        // A minimal impl that only sets `protocol_id` should fall
        // through to defaults that produce UnsupportedOperation. This
        // is the path CBOR-style protocols will rely on.
        #[derive(Debug)]
        struct MinimalSettings;
        impl DocumentSettings for MinimalSettings {
            fn protocol_id(&self) -> &str {
                "com.example#Minimal"
            }
        }

        let s = MinimalSettings;
        assert!(matches!(
            s.coerce_string_to_blob("anything"),
            Err(DocumentError::UnsupportedOperation { .. })
        ));
        assert!(matches!(
            s.coerce_string_to_timestamp("anything"),
            Err(DocumentError::UnsupportedOperation { .. })
        ));
        assert!(matches!(
            s.coerce_number_to_timestamp(&Number::PosInt(0)),
            Err(DocumentError::UnsupportedOperation { .. })
        ));
    }
}
