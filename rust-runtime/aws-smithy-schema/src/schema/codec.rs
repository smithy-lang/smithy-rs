/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Codec trait for creating shape serializers and deserializers.
//!
//! A codec represents a specific serialization format (e.g., JSON, XML, CBOR)
//! and provides methods to create serializers and deserializers for that format.

pub mod http_string;

use crate::serde::{ShapeDeserializer, ShapeSerializer};

/// A codec for a specific serialization format.
///
/// Codecs are responsible for creating [`ShapeSerializer`] and [`ShapeDeserializer`]
/// instances that can serialize and deserialize shapes to and from a specific format.
///
/// # Examples
///
/// Implementing a custom codec:
///
/// ```ignore
/// use aws_smithy_schema::codec::Codec;
/// use aws_smithy_schema::serde::{ShapeSerializer, ShapeDeserializer};
///
/// struct MyCodec {
///     // codec configuration
/// }
///
/// impl Codec for MyCodec {
///     type Serializer = MySerializer;
///     type Deserializer = MyDeserializer;
///
///     fn create_serializer(&self) -> Self::Serializer {
///         MySerializer::new()
///     }
///
///     fn create_deserializer(&self, input: &[u8]) -> Self::Deserializer {
///         MyDeserializer::new(input)
///     }
/// }
/// ```
pub trait Codec {
    /// The serializer type for this codec.
    type Serializer: ShapeSerializer;

    /// The deserializer type for this codec.
    type Deserializer<'a>: ShapeDeserializer;

    /// Creates a new serializer for this codec.
    fn create_serializer(&self) -> Self::Serializer;

    /// Creates a new deserializer for this codec from the given input bytes.
    fn create_deserializer<'a>(&self, input: &'a [u8]) -> Self::Deserializer<'a>;
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::serde::{SerdeError, SerializableStruct, ShapeDeserializer, ShapeSerializer};
    use crate::{prelude::*, Schema};

    // Mock serializer
    struct MockSerializer {
        output: Vec<u8>,
    }

    impl MockSerializer {
        fn finish(self) -> Vec<u8> {
            self.output
        }
    }

    impl ShapeSerializer for MockSerializer {
        fn write_struct(
            &mut self,
            _schema: &Schema,
            _value: &dyn SerializableStruct,
        ) -> Result<(), SerdeError> {
            Ok(())
        }

        fn write_list(
            &mut self,
            _schema: &Schema,
            _write_elements: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
        ) -> Result<(), SerdeError> {
            Ok(())
        }

        fn write_map(
            &mut self,
            _schema: &Schema,
            _write_entries: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
        ) -> Result<(), SerdeError> {
            Ok(())
        }

        fn write_boolean(&mut self, _schema: &Schema, _value: bool) -> Result<(), SerdeError> {
            Ok(())
        }

        fn write_byte(&mut self, _schema: &Schema, _value: i8) -> Result<(), SerdeError> {
            Ok(())
        }

        fn write_short(&mut self, _schema: &Schema, _value: i16) -> Result<(), SerdeError> {
            Ok(())
        }

        fn write_integer(&mut self, _schema: &Schema, _value: i32) -> Result<(), SerdeError> {
            Ok(())
        }

        fn write_long(&mut self, _schema: &Schema, _value: i64) -> Result<(), SerdeError> {
            Ok(())
        }

        fn write_float(&mut self, _schema: &Schema, _value: f32) -> Result<(), SerdeError> {
            Ok(())
        }

        fn write_double(&mut self, _schema: &Schema, _value: f64) -> Result<(), SerdeError> {
            Ok(())
        }

        fn write_big_integer(
            &mut self,
            _schema: &Schema,
            _value: &aws_smithy_types::BigInteger,
        ) -> Result<(), SerdeError> {
            Ok(())
        }

        fn write_big_decimal(
            &mut self,
            _schema: &Schema,
            _value: &aws_smithy_types::BigDecimal,
        ) -> Result<(), SerdeError> {
            Ok(())
        }

        fn write_string(&mut self, _schema: &Schema, _value: &str) -> Result<(), SerdeError> {
            Ok(())
        }

        fn write_blob(
            &mut self,
            _schema: &Schema,
            _value: &aws_smithy_types::Blob,
        ) -> Result<(), SerdeError> {
            Ok(())
        }

        fn write_timestamp(
            &mut self,
            _schema: &Schema,
            _value: &aws_smithy_types::DateTime,
        ) -> Result<(), SerdeError> {
            Ok(())
        }

        fn write_document(
            &mut self,
            _schema: &Schema,
            _value: &aws_smithy_types::Document,
        ) -> Result<(), SerdeError> {
            Ok(())
        }

        fn write_null(&mut self, _schema: &Schema) -> Result<(), SerdeError> {
            Ok(())
        }
    }

    // Mock deserializer
    struct MockDeserializer<'a> {
        #[allow(dead_code)]
        input: &'a [u8],
    }

    impl<'a> ShapeDeserializer for MockDeserializer<'a> {
        fn read_struct<T, F>(
            &mut self,
            _schema: &Schema,
            state: T,
            _consumer: F,
        ) -> Result<T, SerdeError>
        where
            F: FnMut(T, &Schema, &mut Self) -> Result<T, SerdeError>,
        {
            Ok(state)
        }

        fn read_list<T, F>(
            &mut self,
            _schema: &Schema,
            state: T,
            _consumer: F,
        ) -> Result<T, SerdeError>
        where
            F: FnMut(T, &mut Self) -> Result<T, SerdeError>,
        {
            Ok(state)
        }

        fn read_map<T, F>(
            &mut self,
            _schema: &Schema,
            state: T,
            _consumer: F,
        ) -> Result<T, SerdeError>
        where
            F: FnMut(T, String, &mut Self) -> Result<T, SerdeError>,
        {
            Ok(state)
        }

        fn read_boolean(&mut self, _schema: &Schema) -> Result<bool, SerdeError> {
            Ok(false)
        }

        fn read_byte(&mut self, _schema: &Schema) -> Result<i8, SerdeError> {
            Ok(0)
        }

        fn read_short(&mut self, _schema: &Schema) -> Result<i16, SerdeError> {
            Ok(0)
        }

        fn read_integer(&mut self, _schema: &Schema) -> Result<i32, SerdeError> {
            Ok(0)
        }

        fn read_long(&mut self, _schema: &Schema) -> Result<i64, SerdeError> {
            Ok(0)
        }

        fn read_float(&mut self, _schema: &Schema) -> Result<f32, SerdeError> {
            Ok(0.0)
        }

        fn read_double(&mut self, _schema: &Schema) -> Result<f64, SerdeError> {
            Ok(0.0)
        }

        fn read_big_integer(
            &mut self,
            _schema: &Schema,
        ) -> Result<aws_smithy_types::BigInteger, SerdeError> {
            use std::str::FromStr;
            Ok(aws_smithy_types::BigInteger::from_str("0").unwrap())
        }

        fn read_big_decimal(
            &mut self,
            _schema: &Schema,
        ) -> Result<aws_smithy_types::BigDecimal, SerdeError> {
            use std::str::FromStr;
            Ok(aws_smithy_types::BigDecimal::from_str("0").unwrap())
        }

        fn read_string(&mut self, _schema: &Schema) -> Result<String, SerdeError> {
            Ok(String::new())
        }

        fn read_blob(&mut self, _schema: &Schema) -> Result<aws_smithy_types::Blob, SerdeError> {
            Ok(aws_smithy_types::Blob::new(Vec::new()))
        }

        fn read_timestamp(
            &mut self,
            _schema: &Schema,
        ) -> Result<aws_smithy_types::DateTime, SerdeError> {
            Ok(aws_smithy_types::DateTime::from_secs(0))
        }

        fn read_document(
            &mut self,
            _schema: &Schema,
        ) -> Result<aws_smithy_types::Document, SerdeError> {
            Ok(aws_smithy_types::Document::Null)
        }

        fn is_null(&self) -> bool {
            false
        }

        fn container_size(&self) -> Option<usize> {
            None
        }
    }

    // Mock codec
    struct MockCodec;

    impl Codec for MockCodec {
        type Serializer = MockSerializer;
        type Deserializer<'a> = MockDeserializer<'a>;

        fn create_serializer(&self) -> Self::Serializer {
            MockSerializer { output: Vec::new() }
        }

        fn create_deserializer<'a>(&self, input: &'a [u8]) -> Self::Deserializer<'a> {
            MockDeserializer { input }
        }
    }

    #[test]
    fn test_codec_create_serializer() {
        let codec = MockCodec;
        let mut serializer = codec.create_serializer();

        // Test that we can use the serializer
        serializer.write_string(&STRING, "test").unwrap();
        let output = serializer.finish();
        assert_eq!(output, Vec::<u8>::new());
    }

    #[test]
    fn test_codec_create_deserializer() {
        let codec = MockCodec;
        let input = b"test data";
        let mut deserializer = codec.create_deserializer(input);

        // Test that we can use the deserializer
        let result = deserializer.read_string(&STRING).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_codec_roundtrip() {
        let codec = MockCodec;

        // Serialize
        let mut serializer = codec.create_serializer();
        serializer.write_integer(&INTEGER, 42).unwrap();
        let bytes = serializer.finish();

        // Deserialize
        let mut deserializer = codec.create_deserializer(&bytes);
        let value = deserializer.read_integer(&INTEGER).unwrap();
        assert_eq!(value, 0); // Mock deserializer always returns 0
    }
}
