/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! [`DiscriminatedDocumentV2Ext`], the schema-aware methods of
//! [`DiscriminatedDocument<DocumentV2>`].

use aws_smithy_types::{DiscriminatedDocument, DocumentV2};

use super::discriminated_ext::number_shape_type;
use super::{DocumentV2ShapeDeserializer, DocumentV2ShapeSerializer};
use crate::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use crate::{Schema, ShapeType};

/// Schema-aware methods of [`DiscriminatedDocument<DocumentV2>`]: the counterpart of
/// [`DiscriminatedDocumentExt`](super::DiscriminatedDocumentExt), which is implemented for
/// `DiscriminatedDocument<Document>` only.
///
/// The methods have different names from `DiscriminatedDocumentExt`'s so that importing both
/// traits, for example with `use aws_smithy_schema::document::*`, doesn't make a call such as
/// `DiscriminatedDocument::from_struct(...)` ambiguous.
///
/// This trait is sealed: it is implemented for `DiscriminatedDocument<DocumentV2>` only.
///
/// # Example
///
/// ```
/// use aws_smithy_schema::document::DiscriminatedDocumentV2Ext;
/// use aws_smithy_schema::prelude;
/// use aws_smithy_schema::ShapeType;
/// use aws_smithy_types::{DateTime, DiscriminatedDocument, DocumentV2};
///
/// let document = DiscriminatedDocument::new_v2(DocumentV2::Timestamp(DateTime::from_secs(5)));
/// assert_eq!(document.shape_type_v2(), ShapeType::Timestamp);
/// let timestamp = document
///     .as_shape_v2(|deser| deser.read_timestamp(&prelude::TIMESTAMP))
///     .unwrap();
/// assert_eq!(timestamp, DateTime::from_secs(5));
/// ```
pub trait DiscriminatedDocumentV2Ext: sealed::Sealed {
    /// Converts a [`SerializableStruct`] to a [`DiscriminatedDocument<DocumentV2>`] with a
    /// [`DocumentV2ShapeSerializer`].
    ///
    /// This is the SEP's `Document.of(struct)` for `DocumentV2`. Blobs, timestamps, and
    /// arbitrary-precision numbers keep their native variants, and members keep the order in
    /// which `value` writes them. The discriminator is `schema`'s shape ID. No settings are
    /// attached.
    ///
    /// # Errors
    ///
    /// Returns an error if `value`'s `serialize_members` fails, or if `schema` doesn't have an
    /// absolute shape ID (`namespace#Name`). A member schema doesn't.
    fn from_struct_v2(
        schema: &Schema<'_>,
        value: &dyn SerializableStruct,
    ) -> Result<DiscriminatedDocument<DocumentV2>, SerdeError>;

    /// Converts this document to a typed shape by calling `deserialize` with a
    /// [`DocumentV2ShapeDeserializer`].
    ///
    /// This is the SEP's `Document.asShape(...)` for `DocumentV2`. `deserialize` is usually a
    /// generated shape's `deserialize` function. The document's settings, if any, are passed to
    /// the deserializer, so that strings and numbers from a protocol can be read as blobs and
    /// timestamps.
    ///
    /// # Errors
    ///
    /// Returns `deserialize`'s error, typically [`SerdeError::TypeMismatch`] when the document
    /// doesn't match the shape.
    fn as_shape_v2<T, F>(&self, deserialize: F) -> Result<T, SerdeError>
    where
        F: FnOnce(&mut dyn ShapeDeserializer) -> Result<T, SerdeError>;

    /// Returns the [`ShapeType`] of this document's value.
    ///
    /// A number is reported by the SEP's rule for ambiguous numbers, like
    /// [`DiscriminatedDocumentExt::shape_type`](super::DiscriminatedDocumentExt::shape_type):
    /// the first of `Integer`, `Long`, `BigInteger`, and `Double` that holds it. The native
    /// variants are reported as their own types: `Blob`, `Timestamp`, `BigInteger`, and
    /// `BigDecimal`. An object is reported as `Map`, and `Null` as `Document`.
    fn shape_type_v2(&self) -> ShapeType;
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for aws_smithy_types::DiscriminatedDocument<aws_smithy_types::DocumentV2> {}
}

impl DiscriminatedDocumentV2Ext for DiscriminatedDocument<DocumentV2> {
    fn from_struct_v2(
        schema: &Schema<'_>,
        value: &dyn SerializableStruct,
    ) -> Result<DiscriminatedDocument<DocumentV2>, SerdeError> {
        // The inherent `write_struct` records the discriminator.
        let mut ser = DocumentV2ShapeSerializer::new();
        ser.write_struct(schema, value)?;
        ser.finish()
    }

    fn as_shape_v2<T, F>(&self, deserialize: F) -> Result<T, SerdeError>
    where
        F: FnOnce(&mut dyn ShapeDeserializer) -> Result<T, SerdeError>,
    {
        let mut deser = DocumentV2ShapeDeserializer::new_with_settings(
            self.document(),
            self.settings().cloned(),
        );
        deserialize(&mut deser)
    }

    fn shape_type_v2(&self) -> ShapeType {
        match self.document() {
            DocumentV2::Null => ShapeType::Document,
            DocumentV2::Bool(_) => ShapeType::Boolean,
            DocumentV2::Number(n) => number_shape_type(n),
            DocumentV2::String(_) => ShapeType::String,
            DocumentV2::Array(_) => ShapeType::List,
            DocumentV2::Object(_) => ShapeType::Map,
            DocumentV2::Blob(_) => ShapeType::Blob,
            DocumentV2::Timestamp(_) => ShapeType::Timestamp,
            DocumentV2::BigInteger(_) => ShapeType::BigInteger,
            DocumentV2::BigDecimal(_) => ShapeType::BigDecimal,
            // `DocumentV2` is `#[non_exhaustive]`. A future variant is still a document.
            _ => ShapeType::Document,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use aws_smithy_types::document::DocumentObject;
    use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Number};

    use super::*;
    use crate::prelude::{
        BIG_DECIMAL, BIG_INTEGER, BLOB, BOOLEAN, DOCUMENT, DOUBLE, INTEGER, LONG, STRING, TIMESTAMP,
    };
    use crate::serde::ShapeSerializer;
    use crate::{shape_id, ShapeId};

    const BLOBBY_ID: ShapeId<'static> = shape_id!("smithy.example", "Blobby");
    static M_DATA: Schema<'static> = Schema::new_member(
        shape_id!("smithy.example", "Blobby", "data"),
        ShapeType::Blob,
        "data",
        0,
    );
    static BLOBBY_SCHEMA: Schema<'static> =
        Schema::new_struct(BLOBBY_ID, ShapeType::Structure, &[&M_DATA]);

    #[derive(Debug, PartialEq)]
    struct Blobby {
        data: Option<Blob>,
    }

    impl SerializableStruct for Blobby {
        fn schema(&self) -> &Schema<'_> {
            &BLOBBY_SCHEMA
        }

        fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            match &self.data {
                Some(data) => ser.write_blob(&M_DATA, data.clone()),
                None => Ok(()),
            }
        }
    }

    fn deserialize_blobby(deser: &mut dyn ShapeDeserializer) -> Result<Blobby, SerdeError> {
        let mut out = Blobby { data: None };
        deser.read_struct(&BLOBBY_SCHEMA, &mut |member, sub| {
            if member.member_index() == Some(0) {
                out.data = Some(sub.read_blob(member)?);
            }
            Ok(())
        })?;
        Ok(out)
    }

    #[test]
    fn from_struct_v2_keeps_native_values_and_records_the_discriminator() {
        let value = Blobby {
            data: Some(Blob::new(b"raw".to_vec())),
        };
        let document = DiscriminatedDocument::from_struct_v2(&BLOBBY_SCHEMA, &value).unwrap();
        assert_eq!(document.discriminator(), Some("smithy.example#Blobby"));
        assert!(document.settings().is_none());
        assert_eq!(
            document.document(),
            &DocumentV2::Object(DocumentObject::from([(
                "data".to_string(),
                DocumentV2::Blob(b"raw".to_vec())
            )]))
        );
        assert_eq!(document.as_shape_v2(deserialize_blobby).unwrap(), value);
    }

    #[test]
    fn from_struct_v2_rejects_a_member_schema() {
        let value = Blobby { data: None };
        assert!(matches!(
            DiscriminatedDocument::from_struct_v2(&M_DATA, &value),
            Err(SerdeError::InvalidInput { .. })
        ));
    }

    #[test]
    fn as_shape_v2_passes_the_settings_on() {
        #[derive(Debug)]
        struct Utf8Blobs;
        impl aws_smithy_types::DocumentSettings for Utf8Blobs {
            fn protocol_id(&self) -> &str {
                "smithy.example#Utf8Blobs"
            }
            fn coerce_string_to_blob(
                &self,
                s: &str,
            ) -> Result<Vec<u8>, aws_smithy_types::DocumentError> {
                Ok(s.as_bytes().to_vec())
            }
        }

        let document = DiscriminatedDocument::new_v2(DocumentV2::Object(DocumentObject::from([(
            "data".to_string(),
            DocumentV2::from("not base64"),
        )])));
        assert!(document.as_shape_v2(deserialize_blobby).is_err());
        let document = document.with_settings(std::sync::Arc::new(Utf8Blobs));
        assert_eq!(
            document.as_shape_v2(deserialize_blobby).unwrap().data,
            Some(Blob::new(b"not base64".to_vec()))
        );
    }

    /// Every reported type can be read back with the matching read method.
    #[test]
    fn reported_shape_type_can_be_read_back() {
        let cases = [
            (DocumentV2::Null, ShapeType::Document),
            (DocumentV2::Bool(true), ShapeType::Boolean),
            (DocumentV2::from(i64::from(i32::MIN)), ShapeType::Integer),
            (DocumentV2::from(i64::MAX), ShapeType::Long),
            (
                DocumentV2::Number(Number::PosInt(u64::MAX)),
                ShapeType::BigInteger,
            ),
            (DocumentV2::Number(Number::Float(42.0)), ShapeType::Integer),
            (DocumentV2::Number(Number::Float(1.5)), ShapeType::Double),
            (DocumentV2::from("x"), ShapeType::String),
            (DocumentV2::Array(Vec::new()), ShapeType::List),
            (DocumentV2::Object(DocumentObject::new()), ShapeType::Map),
            (DocumentV2::Blob(b"x".to_vec()), ShapeType::Blob),
            (
                DocumentV2::Timestamp(DateTime::from_secs(1)),
                ShapeType::Timestamp,
            ),
            (
                DocumentV2::BigInteger(BigInteger::from_str("5").unwrap()),
                ShapeType::BigInteger,
            ),
            (
                DocumentV2::BigDecimal(BigDecimal::from_str("0.1").unwrap()),
                ShapeType::BigDecimal,
            ),
        ];
        for (value, expected) in cases {
            let document = DiscriminatedDocument::new_v2(value);
            let reported = document.shape_type_v2();
            assert_eq!(reported, expected, "{document:?}");
            let read = document.as_shape_v2(|de| match reported {
                ShapeType::Document => de.read_document_v2(&DOCUMENT).map(|_| ()),
                ShapeType::Boolean => de.read_boolean(&BOOLEAN).map(|_| ()),
                ShapeType::Integer => de.read_integer(&INTEGER).map(|_| ()),
                ShapeType::Long => de.read_long(&LONG).map(|_| ()),
                ShapeType::BigInteger => de.read_big_integer(&BIG_INTEGER).map(|_| ()),
                ShapeType::Double => de.read_double(&DOUBLE).map(|_| ()),
                ShapeType::String => de.read_string(&STRING).map(|_| ()),
                ShapeType::List => de.read_list(&DOCUMENT, &mut |_| Ok(())),
                ShapeType::Map => de.read_map(&DOCUMENT, &mut |_, _| Ok(())),
                ShapeType::Blob => de.read_blob(&BLOB).map(|_| ()),
                ShapeType::Timestamp => de.read_timestamp(&TIMESTAMP).map(|_| ()),
                ShapeType::BigDecimal => de.read_big_decimal(&BIG_DECIMAL).map(|_| ()),
                other => panic!("unexpected report {other:?}"),
            });
            assert!(read.is_ok(), "reported {reported:?}, read failed: {read:?}");
        }
    }
}
