/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! `Document` — a protocol-agnostic representation of any value in the
//! Smithy data model.
//!
//! Documents extend the legacy `aws_smithy_types::Document` to:
//!
//! - Represent the full Smithy data model — including `Blob` and
//!   `Timestamp`, which the legacy type cannot.
//! - Carry an optional `discriminator` (`ShapeId`) so a structure-typed
//!   document can be reified as a concrete shape's data object via a
//!   type registry. (The registry is added in a later commit.)
//! - Carry optional protocol-specific deserialization context via
//!   [`DocumentSettings`] — needed by formats like JSON where blobs are
//!   transmitted as base64 strings and timestamps may be in any of three
//!   string/number formats.
//!
//! See the SEP "Document Type and Type Registries" for the full
//! specification.
//!
//! # Constructing documents
//!
//! Documents are built up from typed constructors:
//!
//! ```
//! use aws_smithy_schema::document::Document;
//! use std::collections::HashMap;
//!
//! let doc = Document::map(HashMap::from([
//!     ("name".to_string(), Document::string("Iago")),
//!     ("age".to_string(), Document::integer(42)),
//! ]));
//! ```
//!
//! # Pattern-matching on documents
//!
//! The underlying [`DocumentInner`] is exposed via [`Document::inner`] for
//! callers that need to pattern-match on the variant. For most use cases,
//! prefer the typed accessors (added in a follow-up commit) which also
//! handle protocol-specific coercions.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use aws_smithy_types::{BigDecimal, BigInteger, DateTime, Number};

use crate::serde::SerdeError;
use crate::{Schema, ShapeId, ShapeType};

/// Protocol-specific deserialization context attached to documents parsed
/// from a wire format.
///
/// Documents can be created in two ways:
///
/// - **Serialize side** — built from a typed shape via
///   `Document::from_struct` or constructed directly by user code. These
///   carry no settings; the default Smithy data-model semantics apply.
///
/// - **Deserialize side** — produced by parsing a wire-format payload (e.g.
///   `JsonDeserializer::read_document`). These carry an
///   `Arc<dyn DocumentSettings>` so the format-specific accessors (most
///   importantly `Document::as_blob` and `Document::as_timestamp`, added
///   in a follow-up commit) can resolve correctly.
///
/// # Implementing this trait
///
/// Each protocol that produces documents from a wire format implements
/// this trait once. The blanket coercion defaults below mirror the SEP's
/// "format-specific coercion" rules — a CBOR-style protocol with native
/// blob and timestamp encodings can leave them at the default (which
/// returns `UnsupportedOperation`), while a JSON-style protocol will
/// override `coerce_string_to_blob` to base64-decode and the timestamp
/// methods to parse the configured `@timestampFormat`.
///
/// `member_index_for` has a default that matches against `member_name`
/// only. Protocols that honor `@jsonName` / `@xmlName` member renaming
/// override this method to consult those traits before falling back to
/// the member name.
///
/// Implementations must be `Debug + Send + Sync` so that documents are
/// safe to share across threads.
pub trait DocumentSettings: std::fmt::Debug + Send + Sync {
    /// The Smithy shape id of the protocol that produced this document
    /// (e.g. `aws.protocols#restJson1`).
    ///
    /// Used for diagnostics and to disambiguate documents that travel
    /// through APIs accepting any protocol.
    fn protocol_id(&self) -> &ShapeId<'static>;

    /// Resolves a wire-level member name to the index of the matching
    /// member in `schema.members()`.
    ///
    /// Returns `None` if no member matches.
    ///
    /// The default implementation matches against
    /// [`Schema::member_name`](crate::Schema::member_name) only.
    /// Protocol implementations that honor member-rename traits
    /// (`@jsonName`, `@xmlName`, etc.) override this method to
    /// consider those traits before falling back to the Smithy member
    /// name.
    fn member_index_for(&self, schema: &Schema, wire_name: &str) -> Option<usize> {
        schema
            .members()
            .iter()
            .position(|m| m.member_name() == Some(wire_name))
    }

    /// Coerces a string value to a blob.
    ///
    /// JSON-style protocols transmit blobs as base64-encoded strings and
    /// override this method to decode. Protocols with a native blob
    /// representation (CBOR, Sparrowhawk) leave this at the default,
    /// which returns [`SerdeError::UnsupportedOperation`].
    fn coerce_string_to_blob(&self, s: &str) -> Result<Vec<u8>, SerdeError> {
        let _ = s;
        Err(SerdeError::UnsupportedOperation {
            message: format!(
                "protocol {} does not support coercing a string to a blob",
                self.protocol_id()
            ),
        })
    }

    /// Coerces a string value to a timestamp.
    ///
    /// JSON-style protocols use this for timestamp formats that encode
    /// as strings (e.g. `date-time`, `http-date`). Protocols that only
    /// transmit timestamps as numbers (e.g. CBOR's tag 1) leave this at
    /// the default.
    fn coerce_string_to_timestamp(&self, s: &str) -> Result<DateTime, SerdeError> {
        let _ = s;
        Err(SerdeError::UnsupportedOperation {
            message: format!(
                "protocol {} does not support coercing a string to a timestamp",
                self.protocol_id()
            ),
        })
    }

    /// Coerces a numeric value to a timestamp.
    ///
    /// Used by JSON-style protocols when the configured timestamp format
    /// is `epoch-seconds`. Protocols that transmit timestamps only as
    /// strings leave this at the default.
    fn coerce_number_to_timestamp(&self, n: &Number) -> Result<DateTime, SerdeError> {
        let _ = n;
        Err(SerdeError::UnsupportedOperation {
            message: format!(
                "protocol {} does not support coercing a number to a timestamp",
                self.protocol_id()
            ),
        })
    }
}

/// A protocol-agnostic representation of any value in the Smithy data model.
///
/// See the module-level documentation for an overview.
#[derive(Clone, Debug)]
pub struct Document {
    inner: DocumentInner,
    /// Optional shape-id discriminator for structure-typed documents.
    ///
    /// Set when a `Document` is constructed from a typed shape (so the
    /// reverse conversion via a type registry can find the right schema)
    /// or when a `Document` is parsed from a payload that carries a
    /// discriminator field (`__type` for JSON).
    // TODO(schema-lifetime): this is what blocks the `__type` lift in
    // `JsonDeserializer::read_document`. Will be relaxed to `ShapeId<'a>`
    // when `Document` itself gains a lifetime parameter, allowing
    // wire-parsed discriminators to live in this slot without heap
    // allocation.
    discriminator: Option<ShapeId<'static>>,
    /// Protocol context for deserialize-side documents. `None` for
    /// serialize-side documents.
    settings: Option<Arc<dyn DocumentSettings>>,
}

/// Inner value of a [`Document`].
///
/// Variants mirror the Smithy data model. The `Number` variant covers
/// `byte`, `short`, `integer`, `long`, `float`, and `double`. Arbitrary-
/// precision `bigInteger` and `bigDecimal` are separate variants because
/// `Number` is bounded by `f64`/`i64`/`u64`.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum DocumentInner {
    /// A null value.
    ///
    /// Null is not a Smithy type proper; it appears in sparse lists and
    /// maps and as a default for missing optional members.
    Null,
    /// A boolean value.
    Boolean(bool),
    /// A bounded numeric value (`byte`, `short`, `integer`, `long`,
    /// `float`, or `double`).
    Number(Number),
    /// An arbitrary-precision integer.
    BigInteger(BigInteger),
    /// An arbitrary-precision decimal.
    BigDecimal(BigDecimal),
    /// A UTF-8 string.
    String(String),
    /// A binary blob.
    Blob(Vec<u8>),
    /// A timestamp.
    Timestamp(DateTime),
    /// An ordered list of documents.
    List(Vec<Document>),
    /// A string-keyed map of documents.
    Map(HashMap<String, Document>),
}

impl Document {
    fn from_inner(inner: DocumentInner) -> Self {
        Self {
            inner,
            discriminator: None,
            settings: None,
        }
    }

    // -- Builders --------------------------------------------------------

    /// Constructs a null document.
    pub fn null() -> Self {
        Self::from_inner(DocumentInner::Null)
    }

    /// Constructs a boolean document.
    pub fn boolean(value: bool) -> Self {
        Self::from_inner(DocumentInner::Boolean(value))
    }

    /// Constructs a string document.
    pub fn string(value: impl Into<String>) -> Self {
        Self::from_inner(DocumentInner::String(value.into()))
    }

    /// Constructs a blob document.
    pub fn blob(value: impl Into<Vec<u8>>) -> Self {
        Self::from_inner(DocumentInner::Blob(value.into()))
    }

    /// Constructs a timestamp document.
    pub fn timestamp(value: DateTime) -> Self {
        Self::from_inner(DocumentInner::Timestamp(value))
    }

    /// Constructs a list document from a `Vec` of documents.
    pub fn list(elements: Vec<Document>) -> Self {
        Self::from_inner(DocumentInner::List(elements))
    }

    /// Constructs a map document from a `HashMap` of documents.
    pub fn map(entries: HashMap<String, Document>) -> Self {
        Self::from_inner(DocumentInner::Map(entries))
    }

    // -- Numeric builders ------------------------------------------------
    //
    // `aws_smithy_types::Number` does not provide `From<iN>` impls, so
    // typed builders are exposed for each Smithy bounded numeric type.

    /// Constructs a numeric document from a Smithy `byte` (`i8`).
    pub fn byte(value: i8) -> Self {
        Self::from_inner(DocumentInner::Number(signed_to_number(value as i64)))
    }

    /// Constructs a numeric document from a Smithy `short` (`i16`).
    pub fn short(value: i16) -> Self {
        Self::from_inner(DocumentInner::Number(signed_to_number(value as i64)))
    }

    /// Constructs a numeric document from a Smithy `integer` (`i32`).
    pub fn integer(value: i32) -> Self {
        Self::from_inner(DocumentInner::Number(signed_to_number(value as i64)))
    }

    /// Constructs a numeric document from a Smithy `long` (`i64`).
    pub fn long(value: i64) -> Self {
        Self::from_inner(DocumentInner::Number(signed_to_number(value)))
    }

    /// Constructs a numeric document from a Smithy `float` (`f32`).
    pub fn float(value: f32) -> Self {
        Self::from_inner(DocumentInner::Number(Number::Float(value as f64)))
    }

    /// Constructs a numeric document from a Smithy `double` (`f64`).
    pub fn double(value: f64) -> Self {
        Self::from_inner(DocumentInner::Number(Number::Float(value)))
    }

    /// Constructs a numeric document from a `Number` directly.
    ///
    /// Useful when working with values produced by a parser that already
    /// produced a `Number`. For typical use cases, prefer the typed
    /// builders ([`Document::byte`], [`Document::integer`], etc.).
    pub fn number(value: Number) -> Self {
        Self::from_inner(DocumentInner::Number(value))
    }

    /// Constructs an arbitrary-precision integer document.
    pub fn big_integer(value: BigInteger) -> Self {
        Self::from_inner(DocumentInner::BigInteger(value))
    }

    /// Constructs an arbitrary-precision decimal document.
    pub fn big_decimal(value: BigDecimal) -> Self {
        Self::from_inner(DocumentInner::BigDecimal(value))
    }

    // -- Discriminator + settings ---------------------------------------

    /// Attaches a shape-id discriminator to this document and returns it.
    ///
    /// Used when constructing a typed document — the discriminator is
    /// what a type registry consults to find the right schema during
    /// the reverse conversion. The SEP requires that documents
    /// constructed from a typed shape preserve this information so the
    /// transformation can round-trip.
    pub fn with_discriminator(mut self, id: ShapeId<'static>) -> Self {
        self.discriminator = Some(id);
        self
    }

    /// Returns the shape-id discriminator if one is attached.
    pub fn discriminator(&self) -> Option<&ShapeId<'static>> {
        self.discriminator.as_ref()
    }

    /// Attaches protocol-specific deserialization settings to this
    /// document.
    ///
    /// Settings carry codec-level context (timestamp format defaults,
    /// `@jsonName` toggle, etc.) that the format-aware accessors
    /// [`Document::as_blob`] and [`Document::as_timestamp`] consult
    /// when coercing from the wire-format representation.
    ///
    /// Settings are typically populated by codec `read_document`
    /// implementations during parsing — see
    /// `aws_smithy_json::codec::JsonDeserializer` for an example.
    /// User code constructing documents from typed shapes via
    /// [`Document::from_struct`] does not attach settings; format
    /// coercion is unnecessary because such documents already carry
    /// native `Blob` / `Timestamp` variants.
    pub fn with_settings(mut self, settings: Arc<dyn DocumentSettings>) -> Self {
        self.settings = Some(settings);
        self
    }

    /// Returns the protocol-specific settings attached to this document,
    /// if any.
    pub fn settings(&self) -> Option<&Arc<dyn DocumentSettings>> {
        self.settings.as_ref()
    }

    /// Returns a reference to the underlying [`DocumentInner`] for callers
    /// that need to pattern-match on the variant directly.
    ///
    /// For the typical use case of "is this a string / what is its value"
    /// prefer the typed accessors (added in a follow-up commit).
    pub fn inner(&self) -> &DocumentInner {
        &self.inner
    }

    // -- High-level entry points ----------------------------------------

    /// Constructs a [`Document`] tree from any
    /// [`SerializableStruct`](crate::serde::SerializableStruct) by
    /// driving it through a `DocumentShapeSerializer`.
    ///
    /// This is the SEP's `Document.of(struct)` entry point. The
    /// resulting document carries `schema.shape_id()` as its
    /// discriminator so the reverse conversion via a type registry can
    /// find the right schema.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let bird_doc = Document::from_struct(Bird::SCHEMA, &my_bird)?;
    /// assert_eq!(bird_doc.discriminator().unwrap().as_str(), "com.example#Bird");
    /// ```
    pub fn from_struct(
        schema: &Schema,
        value: &dyn crate::serde::SerializableStruct,
    ) -> Result<Self, SerdeError> {
        use crate::serde::ShapeSerializer;
        let mut ser = super::DocumentShapeSerializer::new();
        ser.write_struct(schema, value)?;
        ser.finish()
    }

    /// Reifies this [`Document`] as a typed shape by driving the given
    /// `deserialize` callback through a `DocumentShapeDeserializer`.
    ///
    /// This is the SEP's `Document::asShape` entry point. The callback
    /// is typically the generated `<Type>::deserialize` function on a
    /// shape's data carrier or builder; it sees a fresh
    /// [`ShapeDeserializer`](crate::serde::ShapeDeserializer)
    /// positioned at this document.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let bird: Bird = bird_doc.as_shape(|deser| Bird::deserialize(deser))?;
    /// ```
    pub fn as_shape<T, F>(&self, deserialize: F) -> Result<T, SerdeError>
    where
        F: FnOnce(&mut dyn crate::serde::ShapeDeserializer) -> Result<T, SerdeError>,
    {
        let mut deser = super::DocumentShapeDeserializer::new(self);
        deserialize(&mut deser)
    }

    // -- Shape type reporting -------------------------------------------

    /// Returns the [`ShapeType`] this document would be reported as if
    /// converted to a typed shape.
    ///
    /// For values of unambiguous type (boolean, string, blob, timestamp,
    /// list, map, big-integer, big-decimal) this is straightforward.
    ///
    /// For numeric values stored in [`Number`], the SEP "Reporting
    /// `Document` ambiguous shape types" guidance applies: returns the
    /// **first** of `Integer`/`Long`/`BigInteger`/`Double`/`BigDecimal`
    /// that can hold the value without precision loss. `Byte`,
    /// `IntEnum`, `Short`, and `Float` are explicitly skipped.
    ///
    /// `Null` is reported as [`ShapeType::Document`] since `Null` itself
    /// is not a Smithy type variant.
    pub fn shape_type(&self) -> ShapeType {
        match &self.inner {
            DocumentInner::Null => ShapeType::Document,
            DocumentInner::Boolean(_) => ShapeType::Boolean,
            DocumentInner::Number(n) => number_shape_type(n),
            DocumentInner::BigInteger(_) => ShapeType::BigInteger,
            DocumentInner::BigDecimal(_) => ShapeType::BigDecimal,
            DocumentInner::String(_) => ShapeType::String,
            DocumentInner::Blob(_) => ShapeType::Blob,
            DocumentInner::Timestamp(_) => ShapeType::Timestamp,
            DocumentInner::List(_) => ShapeType::List,
            DocumentInner::Map(_) => ShapeType::Map,
        }
    }

    // -- Type-checking accessors ----------------------------------------
    //
    // These return `Option<_>` and never coerce — they are pure type
    // checks. Use the numeric / format-aware accessors below for cases
    // that may need coercion.

    /// Returns the boolean value if this is a `Boolean` document.
    pub fn as_boolean(&self) -> Option<bool> {
        match &self.inner {
            DocumentInner::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    /// Returns the string value if this is a `String` document.
    ///
    /// Does not coerce blobs or timestamps to strings — use
    /// [`Document::as_blob`] / [`Document::as_timestamp`] for those.
    pub fn as_string(&self) -> Option<&str> {
        match &self.inner {
            DocumentInner::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Returns the list elements if this is a `List` document.
    pub fn as_list(&self) -> Option<&[Document]> {
        match &self.inner {
            DocumentInner::List(items) => Some(items.as_slice()),
            _ => None,
        }
    }

    /// Returns the map entries if this is a `Map` document.
    ///
    /// Per the SEP, structure-typed documents (a map with a
    /// [`Document::discriminator`] set) are also accessed via this
    /// method — the keys are the structure's Smithy member names (not
    /// the `@jsonName` / `@xmlName` overrides).
    pub fn as_map(&self) -> Option<&HashMap<String, Document>> {
        match &self.inner {
            DocumentInner::Map(m) => Some(m),
            _ => None,
        }
    }

    /// Returns the document at the given member name if this is a `Map`
    /// document. `None` for any other variant or for a missing key.
    pub fn member(&self, name: &str) -> Option<&Document> {
        self.as_map().and_then(|m| m.get(name))
    }

    /// Returns an iterator over the member names if this is a `Map`
    /// document. `None` for any other variant.
    pub fn member_names(&self) -> Option<impl Iterator<Item = &str>> {
        self.as_map().map(|m| m.keys().map(String::as_str))
    }

    // -- Numeric coercion -----------------------------------------------
    //
    // Per SEP §"Number coercion": numeric accessors automatically
    // convert from the actual numeric type to the requested numeric
    // type. Loss of precision (e.g. converting `3.7` to integer 3) is
    // ignored. Overflow is reported as
    // [`SerdeError::NumericCoercionOverflow`].

    /// Coerces this document's value to a Smithy `byte` (i8).
    ///
    /// Coerces from any numeric variant ([`Number`], [`BigInteger`],
    /// [`BigDecimal`]). Returns [`SerdeError::TypeMismatch`] if this
    /// document is not numeric. Returns
    /// [`SerdeError::NumericCoercionOverflow`] if the value overflows
    /// `i8`.
    pub fn as_byte(&self) -> Result<i8, SerdeError> {
        coerce_signed::<i8>(self, i8::MIN as f64, i8::MAX as f64, "byte")
    }

    /// Coerces this document's value to a Smithy `short` (i16).
    /// See [`Document::as_byte`] for behavior.
    pub fn as_short(&self) -> Result<i16, SerdeError> {
        coerce_signed::<i16>(self, i16::MIN as f64, i16::MAX as f64, "short")
    }

    /// Coerces this document's value to a Smithy `integer` (i32).
    /// See [`Document::as_byte`] for behavior.
    pub fn as_integer(&self) -> Result<i32, SerdeError> {
        coerce_signed::<i32>(self, i32::MIN as f64, i32::MAX as f64, "integer")
    }

    /// Coerces this document's value to a Smithy `long` (i64).
    /// See [`Document::as_byte`] for behavior.
    pub fn as_long(&self) -> Result<i64, SerdeError> {
        coerce_signed::<i64>(self, i64::MIN as f64, i64::MAX as f64, "long")
    }

    /// Coerces this document's value to a Smithy `float` (f32).
    ///
    /// Per the SEP, precision loss (e.g. f64 → f32) is ignored and
    /// over-range values become `±inf`.
    pub fn as_float(&self) -> Result<f32, SerdeError> {
        Ok(self.as_double()? as f32)
    }

    /// Coerces this document's value to a Smithy `double` (f64).
    ///
    /// For [`BigInteger`] / [`BigDecimal`] sources, the value is parsed
    /// via `f64::from_str` (precision loss accepted).
    pub fn as_double(&self) -> Result<f64, SerdeError> {
        match &self.inner {
            DocumentInner::Number(Number::PosInt(v)) => Ok(*v as f64),
            DocumentInner::Number(Number::NegInt(v)) => Ok(*v as f64),
            DocumentInner::Number(Number::Float(f)) => Ok(*f),
            DocumentInner::BigInteger(bi) => bi
                .as_ref()
                .parse::<f64>()
                .map_err(|e| invalid_input("double", bi.as_ref(), &e)),
            DocumentInner::BigDecimal(bd) => bd
                .as_ref()
                .parse::<f64>()
                .map_err(|e| invalid_input("double", bd.as_ref(), &e)),
            other => Err(type_mismatch("double", other)),
        }
    }

    /// Coerces this document's value to a [`BigInteger`].
    ///
    /// Returns the variant directly when it is already a [`BigInteger`].
    /// Coerces from [`Number`] by formatting to a string (always
    /// lossless for integer variants; floats truncate to integer per
    /// SEP "precision loss is ignored"). [`BigDecimal`] sources have
    /// their fractional part truncated.
    pub fn as_big_integer(&self) -> Result<BigInteger, SerdeError> {
        use std::str::FromStr;
        match &self.inner {
            DocumentInner::BigInteger(bi) => Ok(bi.clone()),
            DocumentInner::Number(Number::PosInt(v)) => BigInteger::from_str(&v.to_string())
                .map_err(|e| invalid_input("bigInteger", &v.to_string(), &e)),
            DocumentInner::Number(Number::NegInt(v)) => BigInteger::from_str(&v.to_string())
                .map_err(|e| invalid_input("bigInteger", &v.to_string(), &e)),
            DocumentInner::Number(Number::Float(f)) => {
                if !f.is_finite() {
                    return Err(SerdeError::custom(format!(
                        "cannot coerce non-finite float {f} to bigInteger"
                    )));
                }
                BigInteger::from_str(&(*f as i128).to_string())
                    .map_err(|e| invalid_input("bigInteger", &f.to_string(), &e))
            }
            DocumentInner::BigDecimal(bd) => {
                // Truncate at the decimal point to keep arbitrary
                // precision for the integer portion.
                let s = bd.as_ref();
                let int_part = s.split_once(['.', 'e', 'E']).map(|(i, _)| i).unwrap_or(s);
                BigInteger::from_str(int_part).map_err(|e| invalid_input("bigInteger", s, &e))
            }
            other => Err(type_mismatch("bigInteger", other)),
        }
    }

    /// Coerces this document's value to a [`BigDecimal`].
    ///
    /// Returns the variant directly when it is already a [`BigDecimal`].
    /// Coerces from [`BigInteger`] (the string value is a valid
    /// BigDecimal). Coerces from [`Number`] by formatting to a string.
    pub fn as_big_decimal(&self) -> Result<BigDecimal, SerdeError> {
        use std::str::FromStr;
        match &self.inner {
            DocumentInner::BigDecimal(bd) => Ok(bd.clone()),
            DocumentInner::BigInteger(bi) => BigDecimal::from_str(bi.as_ref())
                .map_err(|e| invalid_input("bigDecimal", bi.as_ref(), &e)),
            DocumentInner::Number(Number::PosInt(v)) => BigDecimal::from_str(&v.to_string())
                .map_err(|e| invalid_input("bigDecimal", &v.to_string(), &e)),
            DocumentInner::Number(Number::NegInt(v)) => BigDecimal::from_str(&v.to_string())
                .map_err(|e| invalid_input("bigDecimal", &v.to_string(), &e)),
            DocumentInner::Number(Number::Float(f)) => {
                if !f.is_finite() {
                    return Err(SerdeError::custom(format!(
                        "cannot coerce non-finite float {f} to bigDecimal"
                    )));
                }
                BigDecimal::from_str(&f.to_string())
                    .map_err(|e| invalid_input("bigDecimal", &f.to_string(), &e))
            }
            other => Err(type_mismatch("bigDecimal", other)),
        }
    }

    // -- Format-aware accessors -----------------------------------------
    //
    // These consult the document's [`DocumentSettings`] for protocol-
    // specific coercion when the variant doesn't match directly. For
    // example a JSON document parsed from a wire format will store
    // blob-typed members as `String` (base64-encoded); calling
    // `as_blob()` on that document returns the decoded bytes via the
    // settings' `coerce_string_to_blob` method.

    /// Returns this document's value as a blob.
    ///
    /// - If the variant is [`DocumentInner::Blob`], returns
    ///   `Cow::Borrowed` of the underlying bytes.
    /// - If the variant is [`DocumentInner::String`] and the document
    ///   carries [`DocumentSettings`], delegates to
    ///   [`DocumentSettings::coerce_string_to_blob`] and returns
    ///   `Cow::Owned`.
    /// - Otherwise returns [`SerdeError::TypeMismatch`] (for non-blob,
    ///   non-string variants) or
    ///   [`SerdeError::UnsupportedOperation`] (for `String` without
    ///   settings).
    pub fn as_blob(&self) -> Result<Cow<'_, [u8]>, SerdeError> {
        match &self.inner {
            DocumentInner::Blob(b) => Ok(Cow::Borrowed(b.as_slice())),
            DocumentInner::String(s) => match &self.settings {
                Some(settings) => settings.coerce_string_to_blob(s).map(Cow::Owned),
                None => Err(SerdeError::UnsupportedOperation {
                    message:
                        "cannot coerce string to blob without protocol-specific document settings"
                            .to_string(),
                }),
            },
            other => Err(type_mismatch("blob", other)),
        }
    }

    /// Returns this document's value as a timestamp.
    ///
    /// - If the variant is [`DocumentInner::Timestamp`], returns it
    ///   directly.
    /// - If the variant is [`DocumentInner::String`] / [`DocumentInner::Number`]
    ///   and the document carries [`DocumentSettings`], delegates to
    ///   [`DocumentSettings::coerce_string_to_timestamp`] /
    ///   [`DocumentSettings::coerce_number_to_timestamp`].
    /// - Otherwise returns an error.
    pub fn as_timestamp(&self) -> Result<DateTime, SerdeError> {
        match (&self.inner, &self.settings) {
            (DocumentInner::Timestamp(t), _) => Ok(*t),
            (DocumentInner::String(s), Some(settings)) => settings.coerce_string_to_timestamp(s),
            (DocumentInner::Number(n), Some(settings)) => settings.coerce_number_to_timestamp(n),
            (DocumentInner::String(_), None) | (DocumentInner::Number(_), None) => {
                Err(SerdeError::UnsupportedOperation {
                    message: "cannot coerce string/number to timestamp without protocol-specific \
                              document settings"
                        .to_string(),
                })
            }
            (other, _) => Err(type_mismatch("timestamp", other)),
        }
    }
}

/// Default value is [`Document::null`].
impl Default for Document {
    fn default() -> Self {
        Self::null()
    }
}

/// Equality compares the inner data and the discriminator. Settings are
/// metadata about how to interpret the data, not the data itself, and are
/// intentionally excluded — a document parsed from a JSON response should
/// compare equal to the same document constructed directly by user code.
impl PartialEq for Document {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner && self.discriminator == other.discriminator
    }
}

// -- Helpers -------------------------------------------------------------

/// Converts a signed `i64` value into the narrowest [`Number`] variant
/// that represents it. Negative values use `Number::NegInt`; non-negative
/// values use `Number::PosInt` (which has a wider positive range).
fn signed_to_number(value: i64) -> Number {
    if value >= 0 {
        Number::PosInt(value as u64)
    } else {
        Number::NegInt(value)
    }
}

/// SEP "Reporting `Document` ambiguous shape types" — for numeric values
/// stored in a [`Number`], picks the narrowest unambiguous container
/// from the precedence order `Integer → Long → BigInteger → Double →
/// BigDecimal`. `Byte`, `IntEnum`, `Short`, and `Float` are skipped.
fn number_shape_type(n: &Number) -> ShapeType {
    match n {
        Number::PosInt(v) => {
            if *v <= i32::MAX as u64 {
                ShapeType::Integer
            } else if *v <= i64::MAX as u64 {
                ShapeType::Long
            } else {
                ShapeType::BigInteger
            }
        }
        Number::NegInt(v) => {
            if *v >= i32::MIN as i64 && *v <= i32::MAX as i64 {
                ShapeType::Integer
            } else {
                // i64 fits in Long but not BigInteger; Number::NegInt is
                // bounded by i64 so BigInteger is unreachable here.
                ShapeType::Long
            }
        }
        Number::Float(v) => {
            // Per the SEP, integer-valued floats should be reported as
            // the narrowest integer container that fits without
            // precision loss. f64 represents integers up to 2^53
            // exactly; beyond that the value is already lossy as f64,
            // so `Double` is the correct report.
            if v.is_finite() && v.fract() == 0.0 {
                if (i32::MIN as f64..=i32::MAX as f64).contains(v) {
                    ShapeType::Integer
                } else if (i64::MIN as f64..=i64::MAX as f64).contains(v) {
                    ShapeType::Long
                } else {
                    ShapeType::Double
                }
            } else {
                ShapeType::Double
            }
        }
    }
}

/// Coerces any numeric `Document` to a signed integer target type.
///
/// Used by [`Document::as_byte`] / [`Document::as_short`] /
/// [`Document::as_integer`] / [`Document::as_long`]. The `min_f64` and
/// `max_f64` bounds are the target's representable range as f64; values
/// outside become [`SerdeError::NumericCoercionOverflow`].
///
/// The generic parameter `T` is the target Rust type. We use
/// `TryFrom<i64>` / `TryFrom<u64>` for integer narrowing (which checks
/// range correctly) and `as` casts for the `Float` variant (after
/// range-checking against the f64 bounds — the SEP says "ignore
/// precision loss" so truncating fractional values is intentional).
fn coerce_signed<T>(doc: &Document, min_f64: f64, max_f64: f64, name: &str) -> Result<T, SerdeError>
where
    T: TryFrom<i64> + TryFrom<u64>,
    f64: NarrowAs<T>,
{
    use std::str::FromStr;
    match &doc.inner {
        DocumentInner::Number(Number::PosInt(v)) => {
            T::try_from(*v).map_err(|_| overflow(name, format_args!("{v}")))
        }
        DocumentInner::Number(Number::NegInt(v)) => {
            T::try_from(*v).map_err(|_| overflow(name, format_args!("{v}")))
        }
        DocumentInner::Number(Number::Float(f)) => narrow_float::<T>(*f, min_f64, max_f64, name),
        DocumentInner::BigInteger(bi) => {
            // First try a direct parse of the BigInteger string into the
            // target type — preserves precision when the value fits.
            // i64::from_str then narrow gives a clean error if the value
            // is too large for the target.
            let parsed = i64::from_str(bi.as_ref())
                .map_err(|_| overflow(name, format_args!("{}", bi.as_ref())))?;
            T::try_from(parsed).map_err(|_| overflow(name, format_args!("{parsed}")))
        }
        DocumentInner::BigDecimal(bd) => {
            // Per SEP "ignore precision loss" — go through f64 and
            // truncate. Values that overflow the target f64 range
            // produce overflow.
            let f = bd
                .as_ref()
                .parse::<f64>()
                .map_err(|e| invalid_input(name, bd.as_ref(), &e))?;
            narrow_float::<T>(f, min_f64, max_f64, name)
        }
        other => Err(type_mismatch(name, other)),
    }
}

/// Helper trait that maps a target signed integer type to the `as` cast
/// used after a range check. We define this as a trait so
/// [`coerce_signed`] can be generic over the target type.
trait NarrowAs<T> {
    fn narrow(self) -> T;
}

impl NarrowAs<i8> for f64 {
    fn narrow(self) -> i8 {
        self as i8
    }
}
impl NarrowAs<i16> for f64 {
    fn narrow(self) -> i16 {
        self as i16
    }
}
impl NarrowAs<i32> for f64 {
    fn narrow(self) -> i32 {
        self as i32
    }
}
impl NarrowAs<i64> for f64 {
    fn narrow(self) -> i64 {
        self as i64
    }
}

/// Range-checks a float against `[min, max]` and narrows to `T` via the
/// trait above. Returns overflow on out-of-range or non-finite inputs.
fn narrow_float<T>(f: f64, min: f64, max: f64, name: &str) -> Result<T, SerdeError>
where
    f64: NarrowAs<T>,
{
    if !f.is_finite() {
        return Err(SerdeError::custom(format!(
            "cannot coerce non-finite float {f} to {name}"
        )));
    }
    if !(min..=max).contains(&f) {
        return Err(overflow(name, format_args!("{f}")));
    }
    Ok(<f64 as NarrowAs<T>>::narrow(f))
}

/// Constructs a `TypeMismatch` error describing the actual variant.
fn type_mismatch(expected: &str, found: &DocumentInner) -> SerdeError {
    let found_name = match found {
        DocumentInner::Null => "null",
        DocumentInner::Boolean(_) => "boolean",
        DocumentInner::Number(_) => "number",
        DocumentInner::BigInteger(_) => "bigInteger",
        DocumentInner::BigDecimal(_) => "bigDecimal",
        DocumentInner::String(_) => "string",
        DocumentInner::Blob(_) => "blob",
        DocumentInner::Timestamp(_) => "timestamp",
        DocumentInner::List(_) => "list",
        DocumentInner::Map(_) => "map",
    };
    SerdeError::TypeMismatch {
        message: format!("expected {expected}, found {found_name}"),
    }
}

/// Constructs a numeric overflow error.
fn overflow(target: &str, value: std::fmt::Arguments<'_>) -> SerdeError {
    SerdeError::NumericCoercionOverflow {
        target: target.to_string(),
        value: value.to_string(),
    }
}

/// Constructs an `InvalidInput` error for parse failures (e.g.
/// malformed BigDecimal as f64).
fn invalid_input(target: &str, value: &str, err: &dyn std::fmt::Display) -> SerdeError {
    SerdeError::InvalidInput {
        message: format!("cannot parse {value:?} as {target}: {err}"),
    }
}

// -- Conversions between the new and legacy Document types ------------
//
// `aws_smithy_types::Document` is the legacy document type without
// `Blob`/`Timestamp`/`BigInteger`/`BigDecimal` variants. The
// schema-serde rollout introduces the richer
// `aws_smithy_schema::document::Document`. These conversions bridge
// the two for code that mixes legacy and schema-serde paths.

/// Convert a legacy [`aws_smithy_types::Document`] into the new
/// [`Document`].
///
/// This is total — every legacy variant has a corresponding new
/// variant, since the legacy type is a strict subset. The new
/// document carries no discriminator and no [`DocumentSettings`];
/// callers can attach a discriminator afterward via
/// [`Document::with_discriminator`].
impl From<aws_smithy_types::Document> for Document {
    fn from(value: aws_smithy_types::Document) -> Self {
        use aws_smithy_types::Document as Legacy;
        match value {
            Legacy::Null => Document::null(),
            Legacy::Bool(b) => Document::boolean(b),
            Legacy::Number(n) => Document::number(n),
            Legacy::String(s) => Document::string(s),
            Legacy::Array(items) => Document::list(items.into_iter().map(Document::from).collect()),
            Legacy::Object(map) => Document::map(
                map.into_iter()
                    .map(|(k, v)| (k, Document::from(v)))
                    .collect(),
            ),
        }
    }
}

/// Convert a new [`Document`] into the legacy
/// [`aws_smithy_types::Document`].
///
/// Fails on variants that have no legacy representation —
/// [`DocumentInner::Blob`], [`DocumentInner::Timestamp`],
/// [`DocumentInner::BigInteger`], and [`DocumentInner::BigDecimal`] —
/// returning [`SerdeError::TypeMismatch`]. The check is recursive: a
/// list or map containing any of those variants fails.
///
/// Discriminator and [`DocumentSettings`] are silently dropped; the
/// legacy type has no slot for them.
impl TryFrom<Document> for aws_smithy_types::Document {
    type Error = SerdeError;

    fn try_from(value: Document) -> Result<Self, Self::Error> {
        use aws_smithy_types::Document as Legacy;
        match value.inner {
            DocumentInner::Null => Ok(Legacy::Null),
            DocumentInner::Boolean(b) => Ok(Legacy::Bool(b)),
            DocumentInner::Number(n) => Ok(Legacy::Number(n)),
            DocumentInner::String(s) => Ok(Legacy::String(s)),
            DocumentInner::List(items) => items
                .into_iter()
                .map(Self::try_from)
                .collect::<Result<Vec<_>, _>>()
                .map(Legacy::Array),
            DocumentInner::Map(map) => map
                .into_iter()
                .map(|(k, v)| Self::try_from(v).map(|v| (k, v)))
                .collect::<Result<HashMap<_, _>, _>>()
                .map(Legacy::Object),
            DocumentInner::Blob(_) => Err(SerdeError::TypeMismatch {
                message: "blob has no representation in aws_smithy_types::Document".to_string(),
            }),
            DocumentInner::Timestamp(_) => Err(SerdeError::TypeMismatch {
                message: "timestamp has no representation in aws_smithy_types::Document"
                    .to_string(),
            }),
            DocumentInner::BigInteger(_) => Err(SerdeError::TypeMismatch {
                message: "bigInteger has no representation in aws_smithy_types::Document"
                    .to_string(),
            }),
            DocumentInner::BigDecimal(_) => Err(SerdeError::TypeMismatch {
                message: "bigDecimal has no representation in aws_smithy_types::Document"
                    .to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for the [`Document`] data shape: builders, accessors,
    //! numeric coercion, format-aware coercion via [`DocumentSettings`],
    //! and [`From`] / [`TryFrom`] bridges to
    //! [`aws_smithy_types::Document`].
    //!
    //! The SEP-normative test cases from
    //! `new-document-type-test-cases.json` exercise the full
    //! `serialize → deserialize → asShape → of` round-trip and depend
    //! on machinery that doesn't exist yet:
    //! - Phase 2: `DocumentShapeSerializer` / `DocumentShapeDeserializer`,
    //!   `Document::from_struct`, `Document::as_shape`.
    //! - Phase 7: `JsonDocumentSettings` (timestamp / blob coercion,
    //!   `__type` discriminator extraction).
    //!
    //! Those normative cases land alongside Phases 2 and 7 per
    //! `.kiro/document-types-and-type-registries-plan.md` §4.

    use super::*;
    use crate::shape_id;

    #[test]
    fn null_document() {
        let d = Document::null();
        assert!(matches!(d.inner(), DocumentInner::Null));
        assert!(d.discriminator().is_none());
        assert!(d.settings().is_none());
    }

    #[test]
    fn boolean_document() {
        let d = Document::boolean(true);
        assert!(matches!(d.inner(), DocumentInner::Boolean(true)));
        assert_eq!(d.shape_type(), ShapeType::Boolean);
    }

    #[test]
    fn string_document() {
        let d = Document::string("hello");
        assert_eq!(d.shape_type(), ShapeType::String);
        if let DocumentInner::String(s) = d.inner() {
            assert_eq!(s, "hello");
        } else {
            panic!("expected String inner");
        }
    }

    #[test]
    fn list_and_map_documents() {
        let list = Document::list(vec![Document::integer(1), Document::integer(2)]);
        assert_eq!(list.shape_type(), ShapeType::List);

        let map = Document::map(HashMap::from([
            ("a".to_string(), Document::integer(1)),
            ("b".to_string(), Document::string("two")),
        ]));
        assert_eq!(map.shape_type(), ShapeType::Map);
    }

    #[test]
    fn blob_and_timestamp_documents() {
        let blob = Document::blob(b"hello".to_vec());
        assert_eq!(blob.shape_type(), ShapeType::Blob);

        let ts = Document::timestamp(DateTime::from_secs(0));
        assert_eq!(ts.shape_type(), ShapeType::Timestamp);
    }

    #[test]
    fn shape_type_pos_int_fits_in_integer() {
        assert_eq!(Document::integer(42).shape_type(), ShapeType::Integer);
        assert_eq!(Document::byte(42).shape_type(), ShapeType::Integer);
        assert_eq!(Document::short(42).shape_type(), ShapeType::Integer);
    }

    #[test]
    fn shape_type_pos_int_overflows_to_long() {
        let just_too_big = (i32::MAX as i64) + 1;
        assert_eq!(Document::long(just_too_big).shape_type(), ShapeType::Long);
    }

    #[test]
    fn shape_type_pos_int_overflows_to_big_integer() {
        // Direct construction with Number::PosInt past i64::MAX.
        let value = (i64::MAX as u64) + 1;
        let d = Document::number(Number::PosInt(value));
        assert_eq!(d.shape_type(), ShapeType::BigInteger);
    }

    #[test]
    fn shape_type_neg_int() {
        assert_eq!(Document::integer(-42).shape_type(), ShapeType::Integer);
        let too_negative = (i32::MIN as i64) - 1;
        assert_eq!(Document::long(too_negative).shape_type(), ShapeType::Long);
    }

    #[test]
    fn shape_type_float_with_integer_value_reports_integer() {
        // 42.0 fits losslessly in i32 → reports Integer per SEP.
        assert_eq!(Document::double(42.0).shape_type(), ShapeType::Integer);
    }

    #[test]
    fn shape_type_non_integer_float_reports_double() {
        assert_eq!(Document::double(1.5).shape_type(), ShapeType::Double);
    }

    #[test]
    fn shape_type_nan_and_infinity_report_double() {
        assert_eq!(Document::double(f64::NAN).shape_type(), ShapeType::Double);
        assert_eq!(
            Document::double(f64::INFINITY).shape_type(),
            ShapeType::Double
        );
    }

    #[test]
    fn shape_type_big_integer_and_big_decimal() {
        let bi: BigInteger = "12345678901234567890123456789".parse().unwrap();
        assert_eq!(
            Document::big_integer(bi).shape_type(),
            ShapeType::BigInteger
        );

        let bd: BigDecimal = "12345678901234567890.123456789012345678901"
            .parse()
            .unwrap();
        assert_eq!(
            Document::big_decimal(bd).shape_type(),
            ShapeType::BigDecimal
        );
    }

    #[test]
    fn discriminator_round_trip() {
        const ID: ShapeId = shape_id!("com.example", "Bird");
        let d = Document::map(HashMap::new()).with_discriminator(ID);
        assert_eq!(d.discriminator(), Some(&ID));
    }

    #[test]
    fn equality_ignores_settings() {
        // Two documents with identical inner+discriminator should compare
        // equal regardless of settings — this is the property the
        // serialize-side / deserialize-side split relies on.
        let a = Document::string("hi");
        let b = Document::string("hi");
        assert_eq!(a, b);

        // Different inner → not equal.
        let c = Document::string("bye");
        assert_ne!(a, c);

        // Different discriminator → not equal.
        const ID_A: ShapeId = shape_id!("com.example", "A");
        const ID_B: ShapeId = shape_id!("com.example", "B");
        let d = Document::null().with_discriminator(ID_A);
        let e = Document::null().with_discriminator(ID_B);
        assert_ne!(d, e);
    }

    #[test]
    fn shape_type_for_null_returns_document() {
        // `Null` has no Smithy simple-type counterpart, so the convention
        // is to report `Document` as the shape type. This is what
        // surrounding deserializers will check when handling sparse
        // collections etc.
        assert_eq!(Document::null().shape_type(), ShapeType::Document);
    }

    #[test]
    fn partial_eq_compares_nested_lists_and_maps_recursively() {
        let nested_a = Document::map(HashMap::from([
            (
                "items".to_string(),
                Document::list(vec![Document::integer(1), Document::string("a")]),
            ),
            ("flag".to_string(), Document::boolean(true)),
        ]));
        let nested_b = Document::map(HashMap::from([
            (
                "items".to_string(),
                Document::list(vec![Document::integer(1), Document::string("a")]),
            ),
            ("flag".to_string(), Document::boolean(true)),
        ]));
        assert_eq!(nested_a, nested_b);

        // A single deep-nested mismatch breaks equality.
        let nested_c = Document::map(HashMap::from([
            (
                "items".to_string(),
                Document::list(vec![Document::integer(1), Document::string("z")]),
            ),
            ("flag".to_string(), Document::boolean(true)),
        ]));
        assert_ne!(nested_a, nested_c);
    }

    #[test]
    fn default_is_null() {
        let d: Document = Default::default();
        assert!(matches!(d.inner(), DocumentInner::Null));
    }

    // --- DocumentSettings trait tests ---

    /// Bare-bones DocumentSettings impl. Overrides nothing beyond the
    /// required `protocol_id`, exercising every default method.
    #[derive(Debug)]
    struct DefaultSettings {
        protocol: ShapeId<'static>,
    }

    impl DocumentSettings for DefaultSettings {
        fn protocol_id(&self) -> &ShapeId<'static> {
            &self.protocol
        }
    }

    #[test]
    fn default_settings_member_index_uses_member_name() {
        // Build a tiny struct schema with two members so we can exercise
        // the default `member_index_for` lookup.
        const ID: ShapeId = shape_id!("com.example", "Bird");
        const NAME: ShapeId = shape_id!("com.example", "Bird", "name");
        const AGE: ShapeId = shape_id!("com.example", "Bird", "age");

        static NAME_MEMBER: Schema = Schema::new_member(NAME, ShapeType::String, "name", 0);
        static AGE_MEMBER: Schema = Schema::new_member(AGE, ShapeType::Integer, "age", 1);
        static BIRD: Schema =
            Schema::new_struct(ID, ShapeType::Structure, &[&NAME_MEMBER, &AGE_MEMBER]);

        let settings = DefaultSettings {
            protocol: shape_id!("com.example", "Test"),
        };

        assert_eq!(settings.member_index_for(&BIRD, "name"), Some(0));
        assert_eq!(settings.member_index_for(&BIRD, "age"), Some(1));
        assert_eq!(settings.member_index_for(&BIRD, "missing"), None);
    }

    #[test]
    fn default_settings_coercion_methods_return_unsupported() {
        let settings = DefaultSettings {
            protocol: shape_id!("com.example", "Test"),
        };

        assert!(matches!(
            settings.coerce_string_to_blob("ignored"),
            Err(SerdeError::UnsupportedOperation { .. })
        ));
        assert!(matches!(
            settings.coerce_string_to_timestamp("1970-01-01T00:00:00Z"),
            Err(SerdeError::UnsupportedOperation { .. })
        ));
        assert!(matches!(
            settings.coerce_number_to_timestamp(&Number::PosInt(0)),
            Err(SerdeError::UnsupportedOperation { .. })
        ));
    }

    #[test]
    fn settings_can_be_attached_to_a_document() {
        // Smoke test that settings round-trip through with_settings →
        // settings(). The actual codecs that populate settings land in
        // Phase 7, so this is enough to confirm the wiring.
        let s: Arc<dyn DocumentSettings> = Arc::new(DefaultSettings {
            protocol: shape_id!("com.example", "Test"),
        });
        let d = Document::null().with_settings(s);
        assert!(d.settings().is_some());
    }

    // --- Type-checking accessor tests ---

    #[test]
    fn as_boolean_returns_some_for_boolean_variant() {
        assert_eq!(Document::boolean(true).as_boolean(), Some(true));
        assert_eq!(Document::boolean(false).as_boolean(), Some(false));
    }

    #[test]
    fn as_boolean_returns_none_for_other_variants() {
        assert_eq!(Document::null().as_boolean(), None);
        assert_eq!(Document::string("true").as_boolean(), None);
        assert_eq!(Document::integer(1).as_boolean(), None);
    }

    #[test]
    fn as_string_returns_some_for_string_variant() {
        assert_eq!(Document::string("hi").as_string(), Some("hi"));
    }

    #[test]
    fn as_string_does_not_coerce() {
        // Unlike as_blob, as_string never coerces from blob — they are
        // distinct Smithy types.
        assert_eq!(Document::blob(b"hi".to_vec()).as_string(), None);
    }

    #[test]
    fn as_list_returns_slice_for_list_variant() {
        let d = Document::list(vec![Document::integer(1), Document::integer(2)]);
        let elems = d.as_list().unwrap();
        assert_eq!(elems.len(), 2);
    }

    #[test]
    fn as_list_returns_none_for_non_list() {
        assert_eq!(Document::null().as_list(), None);
    }

    #[test]
    fn as_map_member_and_member_names() {
        let d = Document::map(HashMap::from([
            ("a".to_string(), Document::integer(1)),
            ("b".to_string(), Document::string("two")),
        ]));
        assert_eq!(d.as_map().unwrap().len(), 2);

        // member() returns the right document, or None for missing.
        assert_eq!(d.member("a").unwrap().as_integer().unwrap(), 1);
        assert_eq!(d.member("missing"), None);

        // member_names returns an iterator over the keys.
        let mut names: Vec<&str> = d.member_names().unwrap().collect();
        names.sort();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn member_returns_none_for_non_map() {
        assert_eq!(Document::null().member("anything"), None);
    }

    // --- Numeric coercion tests ---

    #[test]
    fn as_byte_happy_path_across_source_variants() {
        // Source: Number (PosInt, NegInt, Float)
        assert_eq!(Document::integer(42).as_byte().unwrap(), 42);
        assert_eq!(Document::integer(-42).as_byte().unwrap(), -42);
        assert_eq!(Document::double(42.0).as_byte().unwrap(), 42);
        // Float with fractional → truncates per SEP "ignore precision loss".
        assert_eq!(Document::double(42.7).as_byte().unwrap(), 42);

        // Source: BigInteger / BigDecimal
        let bi = "100".parse::<BigInteger>().unwrap();
        assert_eq!(Document::big_integer(bi).as_byte().unwrap(), 100);

        let bd = "100.5".parse::<BigDecimal>().unwrap();
        assert_eq!(Document::big_decimal(bd).as_byte().unwrap(), 100);
    }

    #[test]
    fn as_byte_overflow_returns_error() {
        // PosInt > i8::MAX
        assert!(Document::integer(200).as_byte().is_err());
        // NegInt < i8::MIN
        assert!(Document::integer(-200).as_byte().is_err());
        // Float out of i8 range
        assert!(Document::double(200.0).as_byte().is_err());
        // BigInteger out of i8 range
        let bi = "200".parse::<BigInteger>().unwrap();
        assert!(Document::big_integer(bi).as_byte().is_err());
    }

    #[test]
    fn as_byte_overflow_returns_typed_variant() {
        // The overflow() helper now emits NumericCoercionOverflow with
        // populated target/value fields rather than SerdeError::Custom.
        let err = Document::integer(200).as_byte().unwrap_err();
        match err {
            SerdeError::NumericCoercionOverflow { target, value } => {
                assert_eq!(target, "byte");
                assert_eq!(value, "200");
            }
            other => panic!("expected NumericCoercionOverflow, got {other:?}"),
        }
    }

    #[test]
    fn as_byte_type_mismatch_for_non_numeric() {
        let err = Document::string("not a number").as_byte().unwrap_err();
        assert!(matches!(err, SerdeError::TypeMismatch { .. }));
    }

    #[test]
    fn as_integer_widens_from_small_sources() {
        // byte → integer (small source, large target — always succeeds).
        assert_eq!(Document::byte(42).as_integer().unwrap(), 42);
        assert_eq!(Document::byte(-1).as_integer().unwrap(), -1);
        // long → integer (overflow possible).
        assert_eq!(
            Document::long(i32::MAX as i64).as_integer().unwrap(),
            i32::MAX
        );
        assert!(Document::long(i32::MAX as i64 + 1).as_integer().is_err());
    }

    #[test]
    fn as_long_handles_max_pos_int() {
        // i64::MAX fits in PosInt(u64); coerces to i64 ok.
        assert_eq!(Document::long(i64::MAX).as_long().unwrap(), i64::MAX);
        // u64::MAX in PosInt overflows i64 → error.
        let too_big = Document::number(Number::PosInt(u64::MAX));
        assert!(too_big.as_long().is_err());
    }

    #[test]
    fn as_double_across_sources() {
        assert_eq!(Document::integer(42).as_double().unwrap(), 42.0);
        assert_eq!(Document::double(2.5).as_double().unwrap(), 2.5);

        let bi = "12345".parse::<BigInteger>().unwrap();
        assert_eq!(Document::big_integer(bi).as_double().unwrap(), 12345.0);

        let bd = "2.5".parse::<BigDecimal>().unwrap();
        assert_eq!(Document::big_decimal(bd).as_double().unwrap(), 2.5);
    }

    #[test]
    fn as_float_inherits_from_as_double() {
        assert_eq!(Document::double(1.5).as_float().unwrap(), 1.5_f32);
    }

    #[test]
    fn as_big_integer_truncates_big_decimal_at_decimal_point() {
        let bd = "12345.678".parse::<BigDecimal>().unwrap();
        let bi = Document::big_decimal(bd).as_big_integer().unwrap();
        assert_eq!(bi.as_ref(), "12345");
    }

    #[test]
    fn as_big_decimal_widens_from_big_integer() {
        let bi = "12345".parse::<BigInteger>().unwrap();
        let bd = Document::big_integer(bi).as_big_decimal().unwrap();
        assert_eq!(bd.as_ref(), "12345");
    }

    // --- Format-aware accessor tests ---

    /// Test settings that base64-decodes strings into blobs and parses
    /// strings as ISO-8601 timestamps. Stands in for what
    /// `JsonDocumentSettings` will do in Phase 7.
    #[derive(Debug)]
    struct TestSettings {
        protocol: ShapeId<'static>,
    }

    impl DocumentSettings for TestSettings {
        fn protocol_id(&self) -> &ShapeId<'static> {
            &self.protocol
        }

        fn coerce_string_to_blob(&self, s: &str) -> Result<Vec<u8>, SerdeError> {
            // Trivial mock: return the raw bytes of the string itself
            // rather than depending on aws-smithy-types' base64 module.
            // The point of this test is to exercise the dispatch, not
            // base64.
            Ok(s.as_bytes().to_vec())
        }

        fn coerce_string_to_timestamp(&self, _s: &str) -> Result<DateTime, SerdeError> {
            // Mock: always return epoch.
            Ok(DateTime::from_secs(0))
        }

        fn coerce_number_to_timestamp(&self, n: &Number) -> Result<DateTime, SerdeError> {
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
            protocol: shape_id!("com.example", "Test"),
        })
    }

    #[test]
    fn as_blob_returns_borrowed_for_blob_variant() {
        let d = Document::blob(b"hi".to_vec());
        match d.as_blob().unwrap() {
            Cow::Borrowed(bytes) => assert_eq!(bytes, b"hi"),
            Cow::Owned(_) => panic!("expected Cow::Borrowed for native Blob"),
        }
    }

    #[test]
    fn as_blob_coerces_string_with_settings() {
        let d = Document::string("hello").with_settings(test_settings());
        match d.as_blob().unwrap() {
            Cow::Owned(bytes) => assert_eq!(bytes, b"hello"),
            Cow::Borrowed(_) => panic!("expected Cow::Owned for coerced String"),
        }
    }

    #[test]
    fn as_blob_string_without_settings_is_unsupported() {
        let err = Document::string("hello").as_blob().unwrap_err();
        assert!(matches!(err, SerdeError::UnsupportedOperation { .. }));
    }

    #[test]
    fn as_blob_type_mismatch_for_non_blob_non_string() {
        let err = Document::integer(42).as_blob().unwrap_err();
        assert!(matches!(err, SerdeError::TypeMismatch { .. }));
    }

    #[test]
    fn as_timestamp_returns_direct_for_timestamp_variant() {
        let ts = DateTime::from_secs(1234);
        let d = Document::timestamp(ts);
        assert_eq!(d.as_timestamp().unwrap(), ts);
    }

    #[test]
    fn as_timestamp_coerces_string_with_settings() {
        let d = Document::string("ignored by mock").with_settings(test_settings());
        assert_eq!(d.as_timestamp().unwrap(), DateTime::from_secs(0));
    }

    #[test]
    fn as_timestamp_coerces_number_with_settings() {
        let d = Document::long(1234).with_settings(test_settings());
        assert_eq!(d.as_timestamp().unwrap(), DateTime::from_secs(1234));
    }

    #[test]
    fn as_timestamp_string_without_settings_is_unsupported() {
        let err = Document::string("ignored").as_timestamp().unwrap_err();
        assert!(matches!(err, SerdeError::UnsupportedOperation { .. }));
    }

    #[test]
    fn as_timestamp_type_mismatch_for_non_coercible() {
        let err = Document::boolean(true).as_timestamp().unwrap_err();
        assert!(matches!(err, SerdeError::TypeMismatch { .. }));
    }

    // --- Conversions between new Document and aws_smithy_types::Document ---

    #[test]
    fn from_legacy_null() {
        let legacy = aws_smithy_types::Document::Null;
        let new: Document = legacy.into();
        assert!(matches!(new.inner(), DocumentInner::Null));
    }

    #[test]
    fn from_legacy_bool_string_number() {
        let legacy = aws_smithy_types::Document::Bool(true);
        assert_eq!(Document::from(legacy).as_boolean(), Some(true));

        let legacy = aws_smithy_types::Document::String("hi".to_string());
        assert_eq!(Document::from(legacy).as_string(), Some("hi"));

        let legacy = aws_smithy_types::Document::Number(Number::PosInt(42));
        assert_eq!(Document::from(legacy).as_integer().unwrap(), 42);
    }

    #[test]
    fn from_legacy_array_recurses() {
        let legacy = aws_smithy_types::Document::Array(vec![
            aws_smithy_types::Document::String("a".to_string()),
            aws_smithy_types::Document::Number(Number::PosInt(1)),
        ]);
        let new = Document::from(legacy);
        let list = new.as_list().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].as_string(), Some("a"));
        assert_eq!(list[1].as_integer().unwrap(), 1);
    }

    #[test]
    fn from_legacy_object_recurses() {
        let mut legacy_map = HashMap::new();
        legacy_map.insert(
            "name".to_string(),
            aws_smithy_types::Document::String("Iago".to_string()),
        );
        legacy_map.insert(
            "age".to_string(),
            aws_smithy_types::Document::Number(Number::PosInt(42)),
        );
        let new = Document::from(aws_smithy_types::Document::Object(legacy_map));

        assert_eq!(new.member("name").unwrap().as_string(), Some("Iago"));
        assert_eq!(new.member("age").unwrap().as_integer().unwrap(), 42);
    }

    #[test]
    fn try_from_to_legacy_simple_variants() {
        let legacy: aws_smithy_types::Document = Document::null().try_into().unwrap();
        assert!(matches!(legacy, aws_smithy_types::Document::Null));

        let legacy: aws_smithy_types::Document = Document::boolean(true).try_into().unwrap();
        assert!(matches!(legacy, aws_smithy_types::Document::Bool(true)));

        let legacy: aws_smithy_types::Document = Document::string("hi").try_into().unwrap();
        assert!(matches!(
            legacy,
            aws_smithy_types::Document::String(ref s) if s == "hi"
        ));

        let legacy: aws_smithy_types::Document = Document::integer(42).try_into().unwrap();
        assert!(matches!(
            legacy,
            aws_smithy_types::Document::Number(Number::PosInt(42))
        ));
    }

    #[test]
    fn try_from_to_legacy_fails_on_extended_variants() {
        let err: SerdeError =
            aws_smithy_types::Document::try_from(Document::blob(b"hi".to_vec())).unwrap_err();
        assert!(matches!(err, SerdeError::TypeMismatch { .. }));

        let err: SerdeError =
            aws_smithy_types::Document::try_from(Document::timestamp(DateTime::from_secs(0)))
                .unwrap_err();
        assert!(matches!(err, SerdeError::TypeMismatch { .. }));

        let bi = "1".parse::<BigInteger>().unwrap();
        let err: SerdeError =
            aws_smithy_types::Document::try_from(Document::big_integer(bi)).unwrap_err();
        assert!(matches!(err, SerdeError::TypeMismatch { .. }));

        let bd = "1.0".parse::<BigDecimal>().unwrap();
        let err: SerdeError =
            aws_smithy_types::Document::try_from(Document::big_decimal(bd)).unwrap_err();
        assert!(matches!(err, SerdeError::TypeMismatch { .. }));
    }

    #[test]
    fn try_from_to_legacy_propagates_through_list_and_map() {
        // List with a nested Blob → fails recursively.
        let list = Document::list(vec![Document::string("ok"), Document::blob(b"!".to_vec())]);
        let err = aws_smithy_types::Document::try_from(list).unwrap_err();
        assert!(matches!(err, SerdeError::TypeMismatch { .. }));

        // Map with a nested Timestamp value → fails recursively.
        let map = Document::map(HashMap::from([
            ("ok".to_string(), Document::string("yes")),
            (
                "when".to_string(),
                Document::timestamp(DateTime::from_secs(0)),
            ),
        ]));
        let err = aws_smithy_types::Document::try_from(map).unwrap_err();
        assert!(matches!(err, SerdeError::TypeMismatch { .. }));
    }

    #[test]
    fn try_from_to_legacy_succeeds_on_nested_compatible() {
        let list = Document::list(vec![
            Document::string("a"),
            Document::list(vec![Document::integer(1), Document::integer(2)]),
        ]);
        let legacy: aws_smithy_types::Document = list.try_into().unwrap();
        match legacy {
            aws_smithy_types::Document::Array(items) => {
                assert_eq!(items.len(), 2);
                assert!(matches!(items[1], aws_smithy_types::Document::Array(_)));
            }
            _ => panic!("expected Array"),
        }
    }

    #[test]
    fn try_from_to_legacy_drops_discriminator() {
        const ID: ShapeId = shape_id!("com.example", "Bird");
        let new = Document::map(HashMap::from([(
            "name".to_string(),
            Document::string("Iago"),
        )]))
        .with_discriminator(ID);
        // Discriminator is silently dropped on conversion. The legacy
        // type has no slot for it.
        let legacy: aws_smithy_types::Document = new.try_into().unwrap();
        assert!(matches!(legacy, aws_smithy_types::Document::Object(_)));
    }

    #[test]
    fn round_trip_legacy_to_new_to_legacy_is_identity_for_compatible_variants() {
        let mut legacy_map = HashMap::new();
        legacy_map.insert(
            "items".to_string(),
            aws_smithy_types::Document::Array(vec![
                aws_smithy_types::Document::String("a".to_string()),
                aws_smithy_types::Document::Number(Number::PosInt(1)),
                aws_smithy_types::Document::Bool(false),
                aws_smithy_types::Document::Null,
            ]),
        );
        let original = aws_smithy_types::Document::Object(legacy_map);

        let new: Document = original.clone().into();
        let round_tripped: aws_smithy_types::Document = new.try_into().unwrap();
        assert_eq!(round_tripped, original);
    }

    // -- from_struct / as_shape entry points ----------------------------

    use crate::serde::{SerializableStruct, ShapeDeserializer, ShapeSerializer};

    const TINY_ID: ShapeId = shape_id!("smithy.example", "Tiny");
    const TINY_FLAG_ID: ShapeId = shape_id!("smithy.example", "Tiny", "flag");
    static TINY_FLAG_MEMBER: Schema =
        Schema::new_member(TINY_FLAG_ID, ShapeType::Boolean, "flag", 0);
    static TINY_SCHEMA: Schema =
        Schema::new_struct(TINY_ID, ShapeType::Structure, &[&TINY_FLAG_MEMBER]);

    #[derive(Debug, PartialEq)]
    struct Tiny {
        flag: bool,
    }

    impl SerializableStruct for Tiny {
        fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            ser.write_boolean(&TINY_FLAG_MEMBER, self.flag)
        }
    }

    fn deserialize_tiny(deser: &mut dyn ShapeDeserializer) -> Result<Tiny, SerdeError> {
        let mut flag = false;
        deser.read_struct(&TINY_SCHEMA, &mut |member, sub| {
            if member.member_index() == Some(0) {
                flag = sub.read_boolean(member)?;
            }
            Ok(())
        })?;
        Ok(Tiny { flag })
    }

    #[test]
    fn from_struct_attaches_discriminator() {
        let doc = Document::from_struct(&TINY_SCHEMA, &Tiny { flag: true }).unwrap();
        assert_eq!(
            doc.discriminator().map(|id| id.as_str()),
            Some("smithy.example#Tiny")
        );
        assert_eq!(
            doc.member("flag").and_then(Document::as_boolean),
            Some(true)
        );
    }

    #[test]
    fn as_shape_round_trips_through_document() {
        let original = Tiny { flag: true };
        let doc = Document::from_struct(&TINY_SCHEMA, &original).unwrap();
        let restored: Tiny = doc.as_shape(deserialize_tiny).unwrap();
        assert_eq!(restored, original);
    }
}
