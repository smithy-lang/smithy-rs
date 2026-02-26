/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Codec trait for creating shape serializers and deserializers.
//!
//! A codec represents a specific serialization format (e.g., JSON, XML, CBOR)
//! and provides methods to create serializers and deserializers for that format.

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
    type Deserializer: ShapeDeserializer;

    /// Creates a new serializer for this codec.
    fn create_serializer(&self) -> Self::Serializer;

    /// Creates a new deserializer for this codec from the given input bytes.
    fn create_deserializer(&self, input: &[u8]) -> Self::Deserializer;
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::serde::{ShapeDeserializer, ShapeSerializer};
    use crate::{prelude::*, Schema};
    use std::fmt;

    // Mock error type
    #[derive(Debug)]
    struct MockError;

    impl fmt::Display for MockError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "mock error")
        }
    }

    impl std::error::Error for MockError {}

    // Mock serializer
    struct MockSerializer {
        output: Vec<u8>,
    }

    impl ShapeSerializer for MockSerializer {
        type Output = Vec<u8>;
        type Error = MockError;

        fn finish(self) -> Result<Self::Output, Self::Error> {
            Ok(self.output)
        }

        fn write_struct<F>(
            &mut self,
            _schema: &dyn Schema,
            _write_members: F,
        ) -> Result<(), Self::Error>
        where
            F: FnOnce(&mut Self) -> Result<(), Self::Error>,
        {
            Ok(())
        }

        fn write_list<F>(
            &mut self,
            _schema: &dyn Schema,
            _write_elements: F,
        ) -> Result<(), Self::Error>
        where
            F: FnOnce(&mut Self) -> Result<(), Self::Error>,
        {
            Ok(())
        }

        fn write_map<F>(
            &mut self,
            _schema: &dyn Schema,
            _write_entries: F,
        ) -> Result<(), Self::Error>
        where
            F: FnOnce(&mut Self) -> Result<(), Self::Error>,
        {
            Ok(())
        }

        fn write_boolean(&mut self, _schema: &dyn Schema, _value: bool) -> Result<(), Self::Error> {
            Ok(())
        }

        fn write_byte(&mut self, _schema: &dyn Schema, _value: i8) -> Result<(), Self::Error> {
            Ok(())
        }

        fn write_short(&mut self, _schema: &dyn Schema, _value: i16) -> Result<(), Self::Error> {
            Ok(())
        }

        fn write_integer(&mut self, _schema: &dyn Schema, _value: i32) -> Result<(), Self::Error> {
            Ok(())
        }

        fn write_long(&mut self, _schema: &dyn Schema, _value: i64) -> Result<(), Self::Error> {
            Ok(())
        }

        fn write_float(&mut self, _schema: &dyn Schema, _value: f32) -> Result<(), Self::Error> {
            Ok(())
        }

        fn write_double(&mut self, _schema: &dyn Schema, _value: f64) -> Result<(), Self::Error> {
            Ok(())
        }

        fn write_big_integer(
            &mut self,
            _schema: &dyn Schema,
            _value: &aws_smithy_types::BigInteger,
        ) -> Result<(), Self::Error> {
            Ok(())
        }

        fn write_big_decimal(
            &mut self,
            _schema: &dyn Schema,
            _value: &aws_smithy_types::BigDecimal,
        ) -> Result<(), Self::Error> {
            Ok(())
        }

        fn write_string(&mut self, _schema: &dyn Schema, _value: &str) -> Result<(), Self::Error> {
            Ok(())
        }

        fn write_blob(
            &mut self,
            _schema: &dyn Schema,
            _value: &aws_smithy_types::Blob,
        ) -> Result<(), Self::Error> {
            Ok(())
        }

        fn write_timestamp(
            &mut self,
            _schema: &dyn Schema,
            _value: &aws_smithy_types::DateTime,
        ) -> Result<(), Self::Error> {
            Ok(())
        }

        fn write_document(
            &mut self,
            _schema: &dyn Schema,
            _value: &aws_smithy_types::Document,
        ) -> Result<(), Self::Error> {
            Ok(())
        }

        fn write_null(&mut self, _schema: &dyn Schema) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    // Mock deserializer
    struct MockDeserializer {
        #[allow(dead_code)]
        input: Vec<u8>,
    }

    impl ShapeDeserializer for MockDeserializer {
        type Error = MockError;

        fn read_struct<T, F>(
            &mut self,
            _schema: &dyn Schema,
            state: T,
            _consumer: F,
        ) -> Result<T, Self::Error>
        where
            F: FnMut(T, &dyn Schema, &mut Self) -> Result<T, Self::Error>,
        {
            Ok(state)
        }

        fn read_list<T, F>(
            &mut self,
            _schema: &dyn Schema,
            state: T,
            _consumer: F,
        ) -> Result<T, Self::Error>
        where
            F: FnMut(T, &mut Self) -> Result<T, Self::Error>,
        {
            Ok(state)
        }

        fn read_map<T, F>(
            &mut self,
            _schema: &dyn Schema,
            state: T,
            _consumer: F,
        ) -> Result<T, Self::Error>
        where
            F: FnMut(T, String, &mut Self) -> Result<T, Self::Error>,
        {
            Ok(state)
        }

        fn read_boolean(&mut self, _schema: &dyn Schema) -> Result<bool, Self::Error> {
            Ok(false)
        }

        fn read_byte(&mut self, _schema: &dyn Schema) -> Result<i8, Self::Error> {
            Ok(0)
        }

        fn read_short(&mut self, _schema: &dyn Schema) -> Result<i16, Self::Error> {
            Ok(0)
        }

        fn read_integer(&mut self, _schema: &dyn Schema) -> Result<i32, Self::Error> {
            Ok(0)
        }

        fn read_long(&mut self, _schema: &dyn Schema) -> Result<i64, Self::Error> {
            Ok(0)
        }

        fn read_float(&mut self, _schema: &dyn Schema) -> Result<f32, Self::Error> {
            Ok(0.0)
        }

        fn read_double(&mut self, _schema: &dyn Schema) -> Result<f64, Self::Error> {
            Ok(0.0)
        }

        fn read_big_integer(
            &mut self,
            _schema: &dyn Schema,
        ) -> Result<aws_smithy_types::BigInteger, Self::Error> {
            use std::str::FromStr;
            Ok(aws_smithy_types::BigInteger::from_str("0").unwrap())
        }

        fn read_big_decimal(
            &mut self,
            _schema: &dyn Schema,
        ) -> Result<aws_smithy_types::BigDecimal, Self::Error> {
            use std::str::FromStr;
            Ok(aws_smithy_types::BigDecimal::from_str("0").unwrap())
        }

        fn read_string(&mut self, _schema: &dyn Schema) -> Result<String, Self::Error> {
            Ok(String::new())
        }

        fn read_blob(
            &mut self,
            _schema: &dyn Schema,
        ) -> Result<aws_smithy_types::Blob, Self::Error> {
            Ok(aws_smithy_types::Blob::new(Vec::new()))
        }

        fn read_timestamp(
            &mut self,
            _schema: &dyn Schema,
        ) -> Result<aws_smithy_types::DateTime, Self::Error> {
            Ok(aws_smithy_types::DateTime::from_secs(0))
        }

        fn read_document(
            &mut self,
            _schema: &dyn Schema,
        ) -> Result<aws_smithy_types::Document, Self::Error> {
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
        type Deserializer = MockDeserializer;

        fn create_serializer(&self) -> Self::Serializer {
            MockSerializer { output: Vec::new() }
        }

        fn create_deserializer(&self, input: &[u8]) -> Self::Deserializer {
            MockDeserializer {
                input: input.to_vec(),
            }
        }
    }

    #[test]
    fn test_codec_create_serializer() {
        let codec = MockCodec;
        let mut serializer = codec.create_serializer();

        // Test that we can use the serializer
        serializer.write_string(&STRING, "test").unwrap();
        let output = serializer.finish().unwrap();
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
        let bytes = serializer.finish().unwrap();

        // Deserialize
        let mut deserializer = codec.create_deserializer(&bytes);
        let value = deserializer.read_integer(&INTEGER).unwrap();
        assert_eq!(value, 0); // Mock deserializer always returns 0
    }
}
