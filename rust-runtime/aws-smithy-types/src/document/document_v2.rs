/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! [`DocumentV2`], the document type for the whole Smithy data model.

mod coercion;
mod conversion;

use super::{DocumentError, DocumentObject};
use crate::{BigDecimal, BigInteger, DateTime, Number};
use std::borrow::Cow;

/// Document type for the whole Smithy data model.
///
/// Document types represent protocol-agnostic open content that is accessed like JSON data.
/// Open content is useful for modeling unstructured data that has no schema, data that can't be
/// modeled using rigid types, or data that has a schema that evolves outside of the purview of a
/// model. The serialization format of a document is an implementation detail of a protocol.
///
/// `DocumentV2` has the JSON-shaped variants of [`Document`](crate::Document)
/// ([`DocumentV2::Object`], [`DocumentV2::Array`], [`DocumentV2::Number`],
/// [`DocumentV2::String`], [`DocumentV2::Bool`], [`DocumentV2::Null`]), plus variants for the
/// Smithy types that JSON can't encode natively ([`DocumentV2::Blob`],
/// [`DocumentV2::Timestamp`], [`DocumentV2::BigInteger`], [`DocumentV2::BigDecimal`]). Its
/// objects keep insertion order.
///
/// `From` converts a `Document` to a `DocumentV2` without losing information.
/// [`DocumentV2::to_document_lossy`] and [`DocumentV2::try_to_document`] convert back. The
/// first encodes the variants that `Document` lacks, and the second returns an error for them.
///
/// `Document` keeps its six variants, because released code matches it exhaustively and a new
/// variant would break that code. `DocumentV2` is `#[non_exhaustive]` so that future additions
/// to the Smithy data model don't require a breaking change. A `match` on a `DocumentV2` outside
/// this crate needs a wildcard arm:
///
/// ```compile_fail,E0004
/// use aws_smithy_types::DocumentV2;
///
/// fn kind(document: &DocumentV2) -> &'static str {
///     match document {
///         DocumentV2::Object(_) => "object",
///         DocumentV2::Array(_) => "array",
///         DocumentV2::Number(_) => "number",
///         DocumentV2::String(_) => "string",
///         DocumentV2::Bool(_) => "bool",
///         DocumentV2::Null => "null",
///         DocumentV2::Blob(_) => "blob",
///         DocumentV2::Timestamp(_) => "timestamp",
///         DocumentV2::BigInteger(_) => "bigInteger",
///         DocumentV2::BigDecimal(_) => "bigDecimal",
///     }
/// }
/// ```
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum DocumentV2 {
    /// JSON object
    Object(DocumentObject),
    /// JSON array
    Array(Vec<DocumentV2>),
    /// JSON number
    Number(Number),
    /// JSON string
    String(String),
    /// JSON boolean
    Bool(bool),
    /// JSON null
    Null,
    /// Smithy blob: a sequence of bytes.
    ///
    /// JSON has no native blob type and sends blobs as base64-encoded strings. A JSON parser
    /// that has no schema therefore produces a [`DocumentV2::String`] for a blob, and
    /// [`DiscriminatedDocument::as_blob`](crate::DiscriminatedDocument::as_blob) decodes it.
    /// This variant holds blobs from formats that encode them natively, such as a CBOR byte
    /// string, and blobs from typed shapes.
    Blob(Vec<u8>),
    /// Smithy timestamp.
    ///
    /// As with [`DocumentV2::Blob`], this variant holds timestamps from formats that encode them
    /// natively, such as CBOR tag 1, and timestamps from typed shapes. A JSON timestamp parsed
    /// without a schema is a [`DocumentV2::String`] or [`DocumentV2::Number`], which
    /// [`DiscriminatedDocument::as_timestamp`](crate::DiscriminatedDocument::as_timestamp)
    /// converts.
    Timestamp(DateTime),
    /// Arbitrary-precision integer.
    ///
    /// Integers that fit in a [`Number`] are usually represented as [`DocumentV2::Number`]. This
    /// variant holds larger integers, and values whose schema requires arbitrary precision.
    BigInteger(BigInteger),
    /// Arbitrary-precision decimal.
    ///
    /// Decimals that an `f64` represents exactly are usually represented as
    /// [`DocumentV2::Number`]. This variant holds other decimals, and values whose schema
    /// requires arbitrary precision.
    BigDecimal(BigDecimal),
}

impl DocumentV2 {
    /// Returns the inner object if this `DocumentV2` is an object.
    pub fn as_object(&self) -> Option<&DocumentObject> {
        if let Self::Object(object) = self {
            Some(object)
        } else {
            None
        }
    }

    /// Returns the mutable inner object if this `DocumentV2` is an object.
    pub fn as_object_mut(&mut self) -> Option<&mut DocumentObject> {
        if let Self::Object(object) = self {
            Some(object)
        } else {
            None
        }
    }

    /// Returns the inner array value if this `DocumentV2` is an array.
    pub fn as_array(&self) -> Option<&Vec<DocumentV2>> {
        if let Self::Array(array) = self {
            Some(array)
        } else {
            None
        }
    }

    /// Returns the mutable inner array value if this `DocumentV2` is an array.
    pub fn as_array_mut(&mut self) -> Option<&mut Vec<DocumentV2>> {
        if let Self::Array(array) = self {
            Some(array)
        } else {
            None
        }
    }

    /// Returns the inner number value if this `DocumentV2` is a number.
    pub fn as_number(&self) -> Option<&Number> {
        if let Self::Number(number) = self {
            Some(number)
        } else {
            None
        }
    }

    /// Returns the inner string value if this `DocumentV2` is a string.
    pub fn as_string(&self) -> Option<&str> {
        if let Self::String(string) = self {
            Some(string)
        } else {
            None
        }
    }

    /// Returns the inner boolean value if this `DocumentV2` is a boolean.
    pub fn as_bool(&self) -> Option<bool> {
        if let Self::Bool(boolean) = self {
            Some(*boolean)
        } else {
            None
        }
    }

    /// Returns `Some(())` if this `DocumentV2` is a null.
    pub fn as_null(&self) -> Option<()> {
        if let Self::Null = self {
            Some(())
        } else {
            None
        }
    }

    /// Returns the inner bytes if this `DocumentV2` is a [`DocumentV2::Blob`].
    ///
    /// This doesn't decode strings. A base64-encoded [`DocumentV2::String`] returns `None`; use
    /// [`DiscriminatedDocument::as_blob`](crate::DiscriminatedDocument::as_blob) to decode it
    /// with the source protocol's rules.
    pub fn as_blob(&self) -> Option<&[u8]> {
        if let Self::Blob(bytes) = self {
            Some(bytes)
        } else {
            None
        }
    }

    /// Returns the mutable inner bytes if this `DocumentV2` is a [`DocumentV2::Blob`].
    pub fn as_blob_mut(&mut self) -> Option<&mut Vec<u8>> {
        if let Self::Blob(bytes) = self {
            Some(bytes)
        } else {
            None
        }
    }

    /// Returns the inner timestamp if this `DocumentV2` is a [`DocumentV2::Timestamp`].
    ///
    /// This doesn't parse strings or numbers. Use
    /// [`DiscriminatedDocument::as_timestamp`](crate::DiscriminatedDocument::as_timestamp)
    /// to convert them with the source protocol's rules.
    pub fn as_timestamp(&self) -> Option<DateTime> {
        if let Self::Timestamp(timestamp) = self {
            Some(*timestamp)
        } else {
            None
        }
    }

    /// Returns the inner value if this `DocumentV2` is a [`DocumentV2::BigInteger`].
    ///
    /// This doesn't convert other numeric variants. Use [`DocumentV2::coerce_big_integer`] for
    /// that.
    pub fn as_big_integer(&self) -> Option<&BigInteger> {
        if let Self::BigInteger(value) = self {
            Some(value)
        } else {
            None
        }
    }

    /// Returns the mutable inner value if this `DocumentV2` is a [`DocumentV2::BigInteger`].
    pub fn as_big_integer_mut(&mut self) -> Option<&mut BigInteger> {
        if let Self::BigInteger(value) = self {
            Some(value)
        } else {
            None
        }
    }

    /// Returns the inner value if this `DocumentV2` is a [`DocumentV2::BigDecimal`].
    ///
    /// This doesn't convert other numeric variants. Use [`DocumentV2::coerce_big_decimal`] for
    /// that.
    pub fn as_big_decimal(&self) -> Option<&BigDecimal> {
        if let Self::BigDecimal(value) = self {
            Some(value)
        } else {
            None
        }
    }

    /// Returns the mutable inner value if this `DocumentV2` is a [`DocumentV2::BigDecimal`].
    pub fn as_big_decimal_mut(&mut self) -> Option<&mut BigDecimal> {
        if let Self::BigDecimal(value) = self {
            Some(value)
        } else {
            None
        }
    }

    /// Returns `true` if this `DocumentV2` is an object.
    pub fn is_object(&self) -> bool {
        matches!(self, Self::Object(_))
    }

    /// Returns `true` if this `DocumentV2` is an array.
    pub fn is_array(&self) -> bool {
        matches!(self, Self::Array(_))
    }

    /// Returns `true` if this `DocumentV2` is a number.
    pub fn is_number(&self) -> bool {
        matches!(self, Self::Number(_))
    }

    /// Returns `true` if this `DocumentV2` is a string.
    pub fn is_string(&self) -> bool {
        matches!(self, Self::String(_))
    }

    /// Returns `true` if this `DocumentV2` is a boolean.
    pub fn is_bool(&self) -> bool {
        matches!(self, Self::Bool(_))
    }

    /// Returns `true` if this `DocumentV2` is a null.
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Returns `true` if this `DocumentV2` is a blob.
    pub fn is_blob(&self) -> bool {
        matches!(self, Self::Blob(_))
    }

    /// Returns `true` if this `DocumentV2` is a timestamp.
    pub fn is_timestamp(&self) -> bool {
        matches!(self, Self::Timestamp(_))
    }

    /// Returns `true` if this `DocumentV2` is a [`DocumentV2::BigInteger`].
    pub fn is_big_integer(&self) -> bool {
        matches!(self, Self::BigInteger(_))
    }

    /// Returns `true` if this `DocumentV2` is a [`DocumentV2::BigDecimal`].
    pub fn is_big_decimal(&self) -> bool {
        matches!(self, Self::BigDecimal(_))
    }

    // -- Numeric coercion -------------------------------------------------
    //
    // These follow the SEP's number coercion rules: every numeric variant converts to every
    // numeric type, loss of precision is ignored, and overflow is an error.

    /// Converts this document's numeric value to a Smithy `byte` (`i8`).
    ///
    /// Accepts every numeric variant. A fractional value is truncated toward zero, so `1.9`
    /// becomes `1` and `-1.9` becomes `-1`. Returns
    /// [`DocumentError::NumericCoercionOverflow`] if the truncated value is out of range,
    /// [`DocumentError::InvalidInput`] for a non-finite float, and
    /// [`DocumentError::TypeMismatch`] for a non-numeric variant.
    pub fn as_byte(&self) -> Result<i8, DocumentError> {
        coercion::to_signed(self, "byte")
    }

    /// Converts this document's numeric value to a Smithy `short` (`i16`).
    ///
    /// See [`DocumentV2::as_byte`] for the conversion rules.
    pub fn as_short(&self) -> Result<i16, DocumentError> {
        coercion::to_signed(self, "short")
    }

    /// Converts this document's numeric value to a Smithy `integer` (`i32`).
    ///
    /// See [`DocumentV2::as_byte`] for the conversion rules.
    pub fn as_integer(&self) -> Result<i32, DocumentError> {
        coercion::to_signed(self, "integer")
    }

    /// Converts this document's numeric value to a Smithy `long` (`i64`).
    ///
    /// See [`DocumentV2::as_byte`] for the conversion rules.
    pub fn as_long(&self) -> Result<i64, DocumentError> {
        coercion::to_signed(self, "long")
    }

    /// Converts this document's numeric value to a Smithy `float` (`f32`).
    ///
    /// Accepts every numeric variant and rounds to the nearest `f32`. Returns
    /// [`DocumentError::NumericCoercionOverflow`] if a finite value is too large for `f32`,
    /// and [`DocumentError::TypeMismatch`] for a non-numeric variant. Non-finite floats are
    /// returned unchanged.
    pub fn as_float(&self) -> Result<f32, DocumentError> {
        coercion::to_f32(self)
    }

    /// Converts this document's numeric value to a Smithy `double` (`f64`).
    ///
    /// Accepts every numeric variant and rounds to the nearest `f64`. Returns
    /// [`DocumentError::NumericCoercionOverflow`] if an arbitrary-precision value is too large
    /// for `f64`, and [`DocumentError::TypeMismatch`] for a non-numeric variant.
    pub fn as_double(&self) -> Result<f64, DocumentError> {
        coercion::to_f64(self)
    }

    /// Converts this document's numeric value to a [`BigInteger`].
    ///
    /// Accepts every numeric variant. A fractional value is truncated toward zero. Returns
    /// [`DocumentError::InvalidInput`] for a non-finite float,
    /// [`DocumentError::NumericCoercionOverflow`] for a decimal whose integer part has more
    /// than 2<sup>20</sup> digits, and [`DocumentError::TypeMismatch`] for a non-numeric
    /// variant.
    pub fn coerce_big_integer(&self) -> Result<BigInteger, DocumentError> {
        coercion::to_big_integer(self)
    }

    /// Converts this document's numeric value to a [`BigDecimal`].
    ///
    /// Accepts every numeric variant. A float converts to its shortest decimal form that
    /// round-trips. Returns [`DocumentError::InvalidInput`] for a non-finite float and
    /// [`DocumentError::TypeMismatch`] for a non-numeric variant.
    pub fn coerce_big_decimal(&self) -> Result<BigDecimal, DocumentError> {
        coercion::to_big_decimal(self)
    }
}

/// The default value is `DocumentV2::Null`.
impl Default for DocumentV2 {
    fn default() -> Self {
        Self::Null
    }
}

impl From<bool> for DocumentV2 {
    fn from(value: bool) -> Self {
        DocumentV2::Bool(value)
    }
}

impl<'a> From<&'a str> for DocumentV2 {
    fn from(value: &'a str) -> Self {
        DocumentV2::String(value.to_string())
    }
}

impl<'a> From<Cow<'a, str>> for DocumentV2 {
    fn from(value: Cow<'a, str>) -> Self {
        DocumentV2::String(value.into_owned())
    }
}

impl From<String> for DocumentV2 {
    fn from(value: String) -> Self {
        DocumentV2::String(value)
    }
}

impl From<Vec<DocumentV2>> for DocumentV2 {
    fn from(values: Vec<DocumentV2>) -> Self {
        DocumentV2::Array(values)
    }
}

impl From<DocumentObject> for DocumentV2 {
    fn from(object: DocumentObject) -> Self {
        DocumentV2::Object(object)
    }
}

impl From<std::collections::HashMap<String, DocumentV2>> for DocumentV2 {
    /// Converts a `HashMap` to a [`DocumentV2::Object`].
    ///
    /// The object's iteration order is the `HashMap`'s iteration order, which is unspecified.
    /// To control the order, build a [`DocumentObject`] with `insert` instead.
    fn from(values: std::collections::HashMap<String, DocumentV2>) -> Self {
        DocumentV2::Object(DocumentObject::from(values))
    }
}

impl From<u64> for DocumentV2 {
    fn from(value: u64) -> Self {
        DocumentV2::Number(Number::PosInt(value))
    }
}

/// Non-negative values become [`Number::PosInt`], because [`Number::NegInt`] only holds
/// negative values. [`Document`](crate::Document)'s conversion produces `NegInt` for every
/// value.
impl From<i64> for DocumentV2 {
    fn from(value: i64) -> Self {
        DocumentV2::Number(signed_number(value))
    }
}

/// Non-negative values become [`Number::PosInt`], because [`Number::NegInt`] only holds
/// negative values. [`Document`](crate::Document)'s conversion produces `NegInt` for every
/// value.
impl From<i32> for DocumentV2 {
    fn from(value: i32) -> Self {
        DocumentV2::from(i64::from(value))
    }
}

impl From<f64> for DocumentV2 {
    fn from(value: f64) -> Self {
        DocumentV2::Number(Number::Float(value))
    }
}

impl From<Number> for DocumentV2 {
    fn from(value: Number) -> Self {
        DocumentV2::Number(value)
    }
}

impl<T> From<Option<T>> for DocumentV2
where
    DocumentV2: From<T>,
{
    fn from(value: Option<T>) -> Self {
        match value {
            Some(inner) => inner.into(),
            None => DocumentV2::Null,
        }
    }
}

/// Returns `value` as a [`Number::PosInt`] if it's non-negative, and as a [`Number::NegInt`]
/// otherwise.
fn signed_number(value: i64) -> Number {
    match u64::try_from(value) {
        Ok(value) => Number::PosInt(value),
        Err(_) => Number::NegInt(value),
    }
}

/// Returns the Smithy name of a [`DocumentV2`] variant, for error messages.
pub(super) fn variant_name(document: &DocumentV2) -> &'static str {
    match document {
        DocumentV2::Object(_) => "object",
        DocumentV2::Array(_) => "array",
        DocumentV2::Number(_) => "number",
        DocumentV2::String(_) => "string",
        DocumentV2::Bool(_) => "boolean",
        DocumentV2::Null => "null",
        DocumentV2::Blob(_) => "blob",
        DocumentV2::Timestamp(_) => "timestamp",
        DocumentV2::BigInteger(_) => "bigInteger",
        DocumentV2::BigDecimal(_) => "bigDecimal",
    }
}

#[cfg(test)]
mod tests {
    use super::{DocumentObject, DocumentV2};
    use crate::{BigDecimal, BigInteger, DateTime, Number};
    use std::str::FromStr;

    #[test]
    fn native_variant_accessors_return_their_values() {
        let blob = DocumentV2::Blob(b"abcd".to_vec());
        assert!(blob.is_blob());
        assert_eq!(blob.as_blob(), Some(b"abcd".as_slice()));

        let timestamp = DateTime::from_secs(1_700_000_000);
        let document = DocumentV2::Timestamp(timestamp);
        assert!(document.is_timestamp());
        assert_eq!(document.as_timestamp(), Some(timestamp));

        let big_integer = BigInteger::from_str("12345678901234567890123").unwrap();
        let document = DocumentV2::BigInteger(big_integer.clone());
        assert!(document.is_big_integer());
        assert_eq!(document.as_big_integer(), Some(&big_integer));

        let big_decimal = BigDecimal::from_str("12345.678").unwrap();
        let document = DocumentV2::BigDecimal(big_decimal.clone());
        assert!(document.is_big_decimal());
        assert_eq!(document.as_big_decimal(), Some(&big_decimal));
    }

    #[test]
    fn accessors_check_the_variant_without_converting() {
        // A base64 string doesn't read as a blob, and the native variants don't read as the
        // JSON-shaped variants that represent them in `Document`.
        let string = DocumentV2::String("YWJjZA==".to_owned());
        assert_eq!(string.as_blob(), None);
        assert_eq!(string.as_timestamp(), None);
        assert_eq!(string.as_big_integer(), None);
        assert_eq!(string.as_big_decimal(), None);

        let blob = DocumentV2::Blob(b"hi".to_vec());
        assert_eq!(blob.as_string(), None);
        assert!(!blob.is_string());

        let big_integer = DocumentV2::BigInteger(BigInteger::from_str("1").unwrap());
        assert_eq!(big_integer.as_number(), None);
        assert!(!big_integer.is_number());
    }

    #[test]
    fn mutable_accessors_modify_in_place() {
        let mut document = DocumentV2::Blob(vec![1, 2, 3]);
        document.as_blob_mut().unwrap().push(4);
        assert_eq!(document.as_blob(), Some([1, 2, 3, 4].as_slice()));

        let mut document = DocumentV2::BigInteger(BigInteger::from_str("0").unwrap());
        *document.as_big_integer_mut().unwrap() = BigInteger::from_str("42").unwrap();
        assert_eq!(document.as_big_integer().unwrap().as_ref(), "42");

        let mut document = DocumentV2::Object(DocumentObject::new());
        document
            .as_object_mut()
            .unwrap()
            .insert("k".to_owned(), DocumentV2::Null);
        assert_eq!(document.as_object().unwrap().len(), 1);
    }

    #[test]
    fn signed_integer_conversions_produce_canonical_numbers() {
        assert_eq!(
            DocumentV2::from(5i64),
            DocumentV2::Number(Number::PosInt(5))
        );
        assert_eq!(
            DocumentV2::from(0i64),
            DocumentV2::Number(Number::PosInt(0))
        );
        assert_eq!(
            DocumentV2::from(-5i64),
            DocumentV2::Number(Number::NegInt(-5))
        );
        assert_eq!(
            DocumentV2::from(i64::MIN),
            DocumentV2::Number(Number::NegInt(i64::MIN))
        );
        assert_eq!(
            DocumentV2::from(7i32),
            DocumentV2::Number(Number::PosInt(7))
        );
        assert_eq!(
            DocumentV2::from(-7i32),
            DocumentV2::Number(Number::NegInt(-7))
        );
    }

    #[test]
    fn json_shaped_conversions_match_document() {
        assert_eq!(DocumentV2::default(), DocumentV2::Null);
        assert_eq!(DocumentV2::from(true), DocumentV2::Bool(true));
        assert_eq!(DocumentV2::from("s"), DocumentV2::String("s".to_owned()));
        assert_eq!(
            DocumentV2::from(std::borrow::Cow::Borrowed("c")),
            DocumentV2::String("c".to_owned())
        );
        assert_eq!(
            DocumentV2::from(1u64),
            DocumentV2::Number(Number::PosInt(1))
        );
        assert_eq!(
            DocumentV2::from(1.5f64),
            DocumentV2::Number(Number::Float(1.5))
        );
        assert_eq!(DocumentV2::from(None::<bool>), DocumentV2::Null);
        assert_eq!(DocumentV2::from(Some(false)), DocumentV2::Bool(false));
        assert!(DocumentV2::from(vec![DocumentV2::Null]).is_array());
        assert!(DocumentV2::from(std::collections::HashMap::new()).is_object());
    }
}
