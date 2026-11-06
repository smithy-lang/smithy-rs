/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Serialization support for [`DocumentV2`]: its [`Serialize`] implementation, and
//! [`to_document_v2`], which converts any `T: Serialize` into a `DocumentV2`.

use super::{signed_number, wide_integer, DocumentV2};
use crate::document::ser::MapKeySerializer;
use crate::document::{DocError, DocumentObject};
use crate::Number;
use serde::ser::{self, Serialize, SerializeMap as _, SerializeSeq as _};
use std::fmt::Display;

/// Writes each variant the way its value's own type serializes.
///
/// The JSON-shaped variants serialize like their [`Document`](crate::Document) counterparts,
/// and objects serialize their entries in order. A blob serializes like
/// [`Blob`](crate::Blob): as a standard base64 string in human-readable formats, and as bytes
/// otherwise. Timestamps serialize like [`DateTime`](crate::DateTime), and arbitrary-precision
/// numbers like [`BigInteger`](crate::BigInteger) and [`BigDecimal`](crate::BigDecimal).
impl Serialize for DocumentV2 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        match self {
            DocumentV2::Object(object) => object.serialize(serializer),
            DocumentV2::Array(values) => {
                let mut seq = serializer.serialize_seq(Some(values.len()))?;
                for value in values {
                    seq.serialize_element(value)?;
                }
                seq.end()
            }
            DocumentV2::Number(number) => number.serialize(serializer),
            DocumentV2::String(string) => serializer.serialize_str(string),
            DocumentV2::Bool(boolean) => serializer.serialize_bool(*boolean),
            DocumentV2::Null => serializer.serialize_unit(),
            DocumentV2::Blob(bytes) => {
                if serializer.is_human_readable() {
                    serializer.serialize_str(&crate::base64::encode(bytes))
                } else {
                    serializer.serialize_bytes(bytes)
                }
            }
            DocumentV2::Timestamp(timestamp) => timestamp.serialize(serializer),
            DocumentV2::BigInteger(value) => value.serialize(serializer),
            DocumentV2::BigDecimal(value) => value.serialize(serializer),
        }
    }
}

impl Serialize for DocumentObject {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.len()))?;
        for (key, value) in self {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

/// Converts any `T: Serialize` into a [`DocumentV2`].
///
/// This works like [`to_document`](crate::document::to_document), with three differences:
///
/// - Objects keep the order in which their entries were serialized, so a struct's fields keep
///   their declaration order.
/// - Values serialized as bytes, such as `serde_bytes::ByteBuf`, become a
///   [`DocumentV2::Blob`]. `to_document` produces an array of numbers.
/// - 128-bit integers become a [`DocumentV2::Number`] if they fit in 64 bits, and a
///   [`DocumentV2::BigInteger`] otherwise. `to_document` returns an error for them.
///
/// The serializer reports itself as human-readable, like `to_document`'s. Types that check,
/// including [`Blob`](crate::Blob), [`DateTime`](crate::DateTime), and the big-number types,
/// therefore serialize as strings, and [`from_document_v2`](crate::document::from_document_v2)
/// reads them back.
///
/// # Example
///
/// ```
/// # #[cfg(all(aws_sdk_unstable, feature = "serde-serialize"))]
/// # {
/// use aws_smithy_types::document::to_document_v2;
/// use serde::Serialize;
///
/// #[derive(Serialize)]
/// struct Account {
///     name: String,
///     balance: u128,
/// }
///
/// let account = Account { name: "Alice".into(), balance: u128::MAX };
/// let document = to_document_v2(&account).unwrap();
/// let object = document.as_object().unwrap();
///
/// let keys: Vec<&str> = object.keys().map(String::as_str).collect();
/// assert_eq!(keys, ["name", "balance"]);
/// assert!(object["balance"].is_big_integer());
/// # }
/// ```
pub fn to_document_v2<T>(value: &T) -> Result<DocumentV2, DocError>
where
    T: ?Sized + Serialize,
{
    value.serialize(DocumentV2Serializer)
}

/// A serializer whose output is a `DocumentV2`.
struct DocumentV2Serializer;

impl ser::Serializer for DocumentV2Serializer {
    type Ok = DocumentV2;
    type Error = DocError;

    type SerializeSeq = SerializeVec;
    type SerializeTuple = SerializeVec;
    type SerializeTupleStruct = SerializeVec;
    type SerializeTupleVariant = SerializeTupleVariant;
    type SerializeMap = SerializeMap;
    type SerializeStruct = SerializeMap;
    type SerializeStructVariant = SerializeStructVariant;

    fn serialize_bool(self, value: bool) -> Result<DocumentV2, DocError> {
        Ok(DocumentV2::Bool(value))
    }

    fn serialize_i8(self, value: i8) -> Result<DocumentV2, DocError> {
        self.serialize_i64(value.into())
    }

    fn serialize_i16(self, value: i16) -> Result<DocumentV2, DocError> {
        self.serialize_i64(value.into())
    }

    fn serialize_i32(self, value: i32) -> Result<DocumentV2, DocError> {
        self.serialize_i64(value.into())
    }

    fn serialize_i64(self, value: i64) -> Result<DocumentV2, DocError> {
        Ok(DocumentV2::Number(signed_number(value)))
    }

    fn serialize_i128(self, value: i128) -> Result<DocumentV2, DocError> {
        wide_integer(value)
    }

    fn serialize_u8(self, value: u8) -> Result<DocumentV2, DocError> {
        self.serialize_u64(value.into())
    }

    fn serialize_u16(self, value: u16) -> Result<DocumentV2, DocError> {
        self.serialize_u64(value.into())
    }

    fn serialize_u32(self, value: u32) -> Result<DocumentV2, DocError> {
        self.serialize_u64(value.into())
    }

    fn serialize_u64(self, value: u64) -> Result<DocumentV2, DocError> {
        Ok(DocumentV2::Number(Number::PosInt(value)))
    }

    fn serialize_u128(self, value: u128) -> Result<DocumentV2, DocError> {
        wide_integer(value)
    }

    fn serialize_f32(self, value: f32) -> Result<DocumentV2, DocError> {
        self.serialize_f64(value.into())
    }

    fn serialize_f64(self, value: f64) -> Result<DocumentV2, DocError> {
        Ok(DocumentV2::Number(Number::Float(value)))
    }

    fn serialize_char(self, value: char) -> Result<DocumentV2, DocError> {
        Ok(DocumentV2::String(value.to_string()))
    }

    fn serialize_str(self, value: &str) -> Result<DocumentV2, DocError> {
        Ok(DocumentV2::String(value.to_owned()))
    }

    fn serialize_bytes(self, value: &[u8]) -> Result<DocumentV2, DocError> {
        Ok(DocumentV2::Blob(value.to_vec()))
    }

    fn serialize_unit(self) -> Result<DocumentV2, DocError> {
        Ok(DocumentV2::Null)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<DocumentV2, DocError> {
        self.serialize_unit()
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<DocumentV2, DocError> {
        self.serialize_str(variant)
    }

    fn serialize_newtype_struct<T>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<DocumentV2, DocError>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<DocumentV2, DocError>
    where
        T: ?Sized + Serialize,
    {
        Ok(single_entry(variant, to_document_v2(value)?))
    }

    fn serialize_none(self) -> Result<DocumentV2, DocError> {
        self.serialize_unit()
    }

    fn serialize_some<T>(self, value: &T) -> Result<DocumentV2, DocError>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<SerializeVec, DocError> {
        Ok(SerializeVec {
            vec: Vec::with_capacity(len.unwrap_or(0)),
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<SerializeVec, DocError> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<SerializeVec, DocError> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<SerializeTupleVariant, DocError> {
        Ok(SerializeTupleVariant {
            name: variant,
            vec: Vec::with_capacity(len),
        })
    }

    fn serialize_map(self, len: Option<usize>) -> Result<SerializeMap, DocError> {
        Ok(SerializeMap {
            object: DocumentObject::with_capacity(len.unwrap_or(0)),
            next_key: None,
        })
    }

    fn serialize_struct(self, _name: &'static str, len: usize) -> Result<SerializeMap, DocError> {
        self.serialize_map(Some(len))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<SerializeStructVariant, DocError> {
        Ok(SerializeStructVariant {
            name: variant,
            object: DocumentObject::with_capacity(len),
        })
    }

    fn collect_str<T>(self, value: &T) -> Result<DocumentV2, DocError>
    where
        T: ?Sized + Display,
    {
        Ok(DocumentV2::String(value.to_string()))
    }
}

/// Returns an object with one entry, which is how enum variants with data serialize.
fn single_entry(key: &str, value: DocumentV2) -> DocumentV2 {
    DocumentV2::Object(DocumentObject::from([(key.to_owned(), value)]))
}

struct SerializeVec {
    vec: Vec<DocumentV2>,
}

struct SerializeTupleVariant {
    name: &'static str,
    vec: Vec<DocumentV2>,
}

struct SerializeMap {
    object: DocumentObject,
    next_key: Option<String>,
}

struct SerializeStructVariant {
    name: &'static str,
    object: DocumentObject,
}

impl ser::SerializeSeq for SerializeVec {
    type Ok = DocumentV2;
    type Error = DocError;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), DocError>
    where
        T: ?Sized + Serialize,
    {
        self.vec.push(to_document_v2(value)?);
        Ok(())
    }

    fn end(self) -> Result<DocumentV2, DocError> {
        Ok(DocumentV2::Array(self.vec))
    }
}

impl ser::SerializeTuple for SerializeVec {
    type Ok = DocumentV2;
    type Error = DocError;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), DocError>
    where
        T: ?Sized + Serialize,
    {
        ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<DocumentV2, DocError> {
        ser::SerializeSeq::end(self)
    }
}

impl ser::SerializeTupleStruct for SerializeVec {
    type Ok = DocumentV2;
    type Error = DocError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), DocError>
    where
        T: ?Sized + Serialize,
    {
        ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<DocumentV2, DocError> {
        ser::SerializeSeq::end(self)
    }
}

impl ser::SerializeTupleVariant for SerializeTupleVariant {
    type Ok = DocumentV2;
    type Error = DocError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), DocError>
    where
        T: ?Sized + Serialize,
    {
        self.vec.push(to_document_v2(value)?);
        Ok(())
    }

    fn end(self) -> Result<DocumentV2, DocError> {
        Ok(single_entry(self.name, DocumentV2::Array(self.vec)))
    }
}

impl ser::SerializeMap for SerializeMap {
    type Ok = DocumentV2;
    type Error = DocError;

    fn serialize_key<T>(&mut self, key: &T) -> Result<(), DocError>
    where
        T: ?Sized + Serialize,
    {
        self.next_key = Some(key.serialize(MapKeySerializer)?);
        Ok(())
    }

    fn serialize_value<T>(&mut self, value: &T) -> Result<(), DocError>
    where
        T: ?Sized + Serialize,
    {
        let key = self
            .next_key
            .take()
            .expect("serialize_value called before serialize_key");
        self.object.insert(key, to_document_v2(value)?);
        Ok(())
    }

    fn end(self) -> Result<DocumentV2, DocError> {
        Ok(DocumentV2::Object(self.object))
    }
}

impl ser::SerializeStruct for SerializeMap {
    type Ok = DocumentV2;
    type Error = DocError;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), DocError>
    where
        T: ?Sized + Serialize,
    {
        ser::SerializeMap::serialize_entry(self, key, value)
    }

    fn end(self) -> Result<DocumentV2, DocError> {
        ser::SerializeMap::end(self)
    }
}

impl ser::SerializeStructVariant for SerializeStructVariant {
    type Ok = DocumentV2;
    type Error = DocError;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), DocError>
    where
        T: ?Sized + Serialize,
    {
        self.object.insert(key.to_owned(), to_document_v2(value)?);
        Ok(())
    }

    fn end(self) -> Result<DocumentV2, DocError> {
        Ok(single_entry(self.name, DocumentV2::Object(self.object)))
    }
}
