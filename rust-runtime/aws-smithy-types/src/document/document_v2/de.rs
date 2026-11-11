/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Deserialization support for [`DocumentV2`]: its [`Deserialize`] implementation, and
//! [`from_document_v2`], which converts a `DocumentV2` into any `T: DeserializeOwned`.

use super::{signed_number, wide_integer, DocumentV2};
use crate::date_time::Format;
use crate::document::de::MapKeyDeserializer;
use crate::document::document_object;
use crate::document::{DocError, DocumentObject};
use crate::Number;
use serde::de::{
    self, DeserializeSeed, EnumAccess, MapAccess, SeqAccess, Unexpected, VariantAccess, Visitor,
};
use serde::{forward_to_deserialize_any, Deserialize};
use std::fmt;

/// Reads any value from a self-describing format.
///
/// JSON-shaped values become the variant with the same name, and objects keep the order of
/// their entries. If a key repeats, its last value is kept at its first position. Bytes from a
/// binary format such as CBOR become a [`DocumentV2::Blob`], and 128-bit integers that don't
/// fit in 64 bits become a [`DocumentV2::BigInteger`].
///
/// This never produces a [`DocumentV2::Timestamp`] or [`DocumentV2::BigDecimal`], because
/// serde has no timestamp or decimal type, and a format's strings stay strings.
impl<'de> Deserialize<'de> for DocumentV2 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_any(DocumentV2Visitor)
    }
}

impl<'de> Deserialize<'de> for DocumentObject {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_map(DocumentObjectVisitor)
    }
}

/// Limits preallocation to 1 MiB, so a size hint from untrusted input can't force a large
/// allocation before any element arrives.
pub(super) fn cautious_capacity<T>(hint: Option<usize>) -> usize {
    const MAX_PREALLOCATED_BYTES: usize = 1024 * 1024;
    hint.unwrap_or(0)
        .min(MAX_PREALLOCATED_BYTES / std::mem::size_of::<T>().max(1))
}

struct DocumentV2Visitor;

impl<'de> Visitor<'de> for DocumentV2Visitor {
    type Value = DocumentV2;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any document value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<DocumentV2, E> {
        Ok(DocumentV2::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<DocumentV2, E> {
        Ok(DocumentV2::Number(signed_number(value)))
    }

    fn visit_i128<E: de::Error>(self, value: i128) -> Result<DocumentV2, E> {
        wide_integer(value).map_err(E::custom)
    }

    fn visit_u64<E>(self, value: u64) -> Result<DocumentV2, E> {
        Ok(DocumentV2::Number(Number::PosInt(value)))
    }

    fn visit_u128<E: de::Error>(self, value: u128) -> Result<DocumentV2, E> {
        wide_integer(value).map_err(E::custom)
    }

    fn visit_f64<E>(self, value: f64) -> Result<DocumentV2, E> {
        Ok(DocumentV2::Number(Number::Float(value)))
    }

    fn visit_str<E>(self, value: &str) -> Result<DocumentV2, E> {
        Ok(DocumentV2::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<DocumentV2, E> {
        Ok(DocumentV2::String(value))
    }

    fn visit_bytes<E>(self, value: &[u8]) -> Result<DocumentV2, E> {
        Ok(DocumentV2::Blob(value.to_vec()))
    }

    fn visit_byte_buf<E>(self, value: Vec<u8>) -> Result<DocumentV2, E> {
        Ok(DocumentV2::Blob(value))
    }

    fn visit_none<E>(self) -> Result<DocumentV2, E> {
        Ok(DocumentV2::Null)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<DocumentV2, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        DocumentV2::deserialize(deserializer)
    }

    fn visit_unit<E>(self) -> Result<DocumentV2, E> {
        Ok(DocumentV2::Null)
    }

    fn visit_newtype_struct<D>(self, deserializer: D) -> Result<DocumentV2, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        DocumentV2::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut access: A) -> Result<DocumentV2, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::with_capacity(cautious_capacity::<DocumentV2>(access.size_hint()));
        while let Some(value) = access.next_element()? {
            values.push(value);
        }
        Ok(DocumentV2::Array(values))
    }

    fn visit_map<A>(self, access: A) -> Result<DocumentV2, A::Error>
    where
        A: MapAccess<'de>,
    {
        DocumentObjectVisitor
            .visit_map(access)
            .map(DocumentV2::Object)
    }
}

struct DocumentObjectVisitor;

impl<'de> Visitor<'de> for DocumentObjectVisitor {
    type Value = DocumentObject;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a document object")
    }

    fn visit_map<A>(self, mut access: A) -> Result<DocumentObject, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut object = DocumentObject::with_capacity(cautious_capacity::<(String, DocumentV2)>(
            access.size_hint(),
        ));
        while let Some((key, value)) = access.next_entry()? {
            object.insert(key, value);
        }
        Ok(object)
    }
}

/// Converts a [`DocumentV2`] into any `T: DeserializeOwned`.
///
/// This works like [`from_document`](crate::document::from_document). `T` reads the document
/// as it would read the document's own serialization in a human-readable format such as JSON,
/// so the variants that `Document` lacks are read as text:
///
/// - A blob is read as standard base64 text, which [`Blob`](crate::Blob) decodes. A type that
///   asks for bytes, such as `serde_bytes::ByteBuf`, gets the bytes instead.
/// - A timestamp is read as an RFC 3339 `date-time` string, which
///   [`DateTime`](crate::DateTime) parses.
/// - An arbitrary-precision number is read as its text, which
///   [`BigInteger`](crate::BigInteger) and [`BigDecimal`](crate::BigDecimal) parse. An `i128`
///   or `u128` can also read a big integer that's in its range.
///
/// Reading into `DocumentV2` therefore turns those variants into strings, as a JSON round trip
/// does.
///
/// Returns an error if `T` doesn't accept the value, or if a timestamp is outside the range
/// that an RFC 3339 `date-time` can represent.
///
/// # Example
///
/// ```
/// # #[cfg(all(aws_sdk_unstable, feature = "serde-deserialize"))]
/// # {
/// use aws_smithy_types::document::{from_document_v2, DocumentObject};
/// use aws_smithy_types::{Blob, DateTime, DocumentV2};
/// use serde::Deserialize;
///
/// #[derive(Deserialize, Debug, PartialEq)]
/// struct Upload {
///     data: Blob,
///     uploaded: DateTime,
/// }
///
/// let document = DocumentV2::Object(DocumentObject::from([
///     ("data".to_owned(), DocumentV2::Blob(b"hello".to_vec())),
///     ("uploaded".to_owned(), DocumentV2::Timestamp(DateTime::from_secs(0))),
/// ]));
/// let upload: Upload = from_document_v2(document).unwrap();
/// assert_eq!(upload.data, Blob::new(b"hello".to_vec()));
/// assert_eq!(upload.uploaded, DateTime::from_secs(0));
/// # }
/// ```
pub fn from_document_v2<T>(document: DocumentV2) -> Result<T, DocError>
where
    T: de::DeserializeOwned,
{
    T::deserialize(document)
}

/// Formats a timestamp the way [`DateTime`](crate::DateTime)'s human-readable serialization
/// does.
fn date_time_text(timestamp: crate::DateTime) -> Result<String, DocError> {
    timestamp
        .fmt(Format::DateTime)
        .map_err(|e| DocError::custom(format_args!("cannot format timestamp: {e}")))
}

impl<'de> de::Deserializer<'de> for DocumentV2 {
    type Error = DocError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, DocError>
    where
        V: Visitor<'de>,
    {
        match self {
            DocumentV2::Null => visitor.visit_unit(),
            DocumentV2::Bool(boolean) => visitor.visit_bool(boolean),
            DocumentV2::Number(number) => match number {
                Number::PosInt(value) => visitor.visit_u64(value),
                Number::NegInt(value) => visitor.visit_i64(value),
                Number::Float(value) => visitor.visit_f64(value),
            },
            DocumentV2::String(string) => visitor.visit_string(string),
            DocumentV2::Array(values) => {
                let mut access = SeqDeserializer {
                    iter: values.into_iter(),
                };
                let value = visitor.visit_seq(&mut access)?;
                if access.iter.len() == 0 {
                    Ok(value)
                } else {
                    Err(DocError::custom(format_args!(
                        "expected end of sequence, got {} remaining elements",
                        access.iter.len()
                    )))
                }
            }
            DocumentV2::Object(object) => {
                let mut access = MapDeserializer {
                    iter: object.into_iter(),
                    value: None,
                };
                let value = visitor.visit_map(&mut access)?;
                if access.iter.len() == 0 {
                    Ok(value)
                } else {
                    Err(DocError::custom(format_args!(
                        "expected end of map, got {} remaining entries",
                        access.iter.len()
                    )))
                }
            }
            DocumentV2::Blob(bytes) => visitor.visit_string(crate::base64::encode(bytes)),
            DocumentV2::Timestamp(timestamp) => visitor.visit_string(date_time_text(timestamp)?),
            DocumentV2::BigInteger(value) => visitor.visit_str(value.as_ref()),
            DocumentV2::BigDecimal(value) => visitor.visit_str(value.as_ref()),
        }
    }

    /// Reads a blob as its bytes. Other variants are read as by `deserialize_any`.
    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, DocError>
    where
        V: Visitor<'de>,
    {
        self.deserialize_byte_buf(visitor)
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, DocError>
    where
        V: Visitor<'de>,
    {
        match self {
            DocumentV2::Blob(bytes) => visitor.visit_byte_buf(bytes),
            other => other.deserialize_any(visitor),
        }
    }

    fn deserialize_i128<V>(self, visitor: V) -> Result<V::Value, DocError>
    where
        V: Visitor<'de>,
    {
        match self {
            DocumentV2::BigInteger(value) => match value.as_ref().parse() {
                Ok(parsed) => visitor.visit_i128(parsed),
                Err(_) => Err(de::Error::invalid_value(
                    Unexpected::Str(value.as_ref()),
                    &visitor,
                )),
            },
            other => other.deserialize_any(visitor),
        }
    }

    fn deserialize_u128<V>(self, visitor: V) -> Result<V::Value, DocError>
    where
        V: Visitor<'de>,
    {
        match self {
            DocumentV2::BigInteger(value) => match value.as_ref().parse() {
                Ok(parsed) => visitor.visit_u128(parsed),
                Err(_) => Err(de::Error::invalid_value(
                    Unexpected::Str(value.as_ref()),
                    &visitor,
                )),
            },
            other => other.deserialize_any(visitor),
        }
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, DocError>
    where
        V: Visitor<'de>,
    {
        match self {
            DocumentV2::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, DocError>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, DocError>
    where
        V: Visitor<'de>,
    {
        match self {
            DocumentV2::String(variant) => visitor.visit_enum(EnumDeserializer {
                variant,
                value: None,
            }),
            DocumentV2::Object(object) => {
                let mut entries = object.into_iter();
                match (entries.next(), entries.next()) {
                    (Some((variant, value)), None) => visitor.visit_enum(EnumDeserializer {
                        variant,
                        value: Some(value),
                    }),
                    (Some(_), Some(_)) => Err(DocError::custom(
                        "expected a single-entry map for enum variant",
                    )),
                    (None, _) => Err(DocError::custom(
                        "expected a non-empty map for enum variant",
                    )),
                }
            }
            _ => Err(DocError::custom(
                "expected a string or map for enum deserialization",
            )),
        }
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string unit
        unit_struct seq tuple tuple_struct map struct identifier ignored_any
    }
}

/// Deserializer for array elements.
struct SeqDeserializer {
    iter: std::vec::IntoIter<DocumentV2>,
}

impl<'de> SeqAccess<'de> for SeqDeserializer {
    type Error = DocError;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, DocError>
    where
        T: DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some(value) => seed.deserialize(value).map(Some),
            None => Ok(None),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.iter.len())
    }
}

/// Deserializer for object entries.
struct MapDeserializer {
    iter: document_object::IntoIter,
    value: Option<DocumentV2>,
}

impl<'de> MapAccess<'de> for MapDeserializer {
    type Error = DocError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, DocError>
    where
        K: DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some((key, value)) => {
                self.value = Some(value);
                seed.deserialize(MapKeyDeserializer { key }).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, DocError>
    where
        V: DeserializeSeed<'de>,
    {
        let value = self
            .value
            .take()
            .expect("next_value_seed called before next_key_seed");
        seed.deserialize(value)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.iter.len())
    }
}

/// Deserializer for an enum: a variant name, with the variant's data if it has any.
struct EnumDeserializer {
    variant: String,
    value: Option<DocumentV2>,
}

impl<'de> EnumAccess<'de> for EnumDeserializer {
    type Error = DocError;
    type Variant = VariantDeserializer;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, VariantDeserializer), DocError>
    where
        V: DeserializeSeed<'de>,
    {
        let variant = seed.deserialize(DocumentV2::String(self.variant))?;
        Ok((variant, VariantDeserializer { value: self.value }))
    }
}

/// Deserializer for an enum variant's data.
struct VariantDeserializer {
    value: Option<DocumentV2>,
}

impl<'de> VariantAccess<'de> for VariantDeserializer {
    type Error = DocError;

    fn unit_variant(self) -> Result<(), DocError> {
        match self.value {
            None | Some(DocumentV2::Null) => Ok(()),
            Some(_) => Err(DocError::custom("expected unit variant, found value")),
        }
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, DocError>
    where
        T: DeserializeSeed<'de>,
    {
        match self.value {
            Some(value) => seed.deserialize(value),
            None => Err(DocError::custom(
                "expected newtype variant, found unit variant",
            )),
        }
    }

    fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value, DocError>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Some(DocumentV2::Array(values)) => visitor.visit_seq(&mut SeqDeserializer {
                iter: values.into_iter(),
            }),
            Some(_) => Err(DocError::custom("expected array for tuple variant")),
            None => Err(DocError::custom(
                "expected tuple variant, found unit variant",
            )),
        }
    }

    fn struct_variant<V>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, DocError>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Some(DocumentV2::Object(object)) => visitor.visit_map(&mut MapDeserializer {
                iter: object.into_iter(),
                value: None,
            }),
            Some(_) => Err(DocError::custom("expected object for struct variant")),
            None => Err(DocError::custom(
                "expected struct variant, found unit variant",
            )),
        }
    }
}
