/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! JSON deserializer implementation.

use aws_smithy_schema::serde::ShapeDeserializer;
use aws_smithy_schema::Schema;
use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Document, Number};
use std::fmt;

use crate::codec::JsonCodecSettings;
use crate::deserialize::{json_token_iter, Token};

/// Error type for JSON deserialization.
#[derive(Debug)]
pub enum JsonDeserializerError {
    /// An error occurred during JSON parsing.
    ParseError(String),
}

impl fmt::Display for JsonDeserializerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ParseError(msg) => write!(f, "JSON parse error: {}", msg),
        }
    }
}

impl std::error::Error for JsonDeserializerError {}

/// JSON deserializer that implements the ShapeDeserializer trait.
pub struct JsonDeserializer {
    input: Vec<u8>,
    position: usize,
    _settings: JsonCodecSettings,
}

impl JsonDeserializer {
    /// Creates a new JSON deserializer with the given settings.
    pub fn new(input: &[u8], settings: JsonCodecSettings) -> Self {
        Self {
            input: input.to_vec(),
            position: 0,
            _settings: settings,
        }
    }

    fn tokens(&self) -> crate::deserialize::JsonTokenIterator<'_> {
        json_token_iter(&self.input[self.position..])
    }
}

impl ShapeDeserializer for JsonDeserializer {
    type Error = JsonDeserializerError;

    fn read_struct<T, F>(
        &mut self,
        _schema: &dyn Schema,
        state: T,
        _consumer: F,
    ) -> Result<T, Self::Error>
    where
        F: FnMut(T, &dyn Schema, &mut Self) -> Result<T, Self::Error>,
    {
        // Minimal implementation - full implementation will come later
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
        let mut tokens = self.tokens();
        match tokens.next() {
            Some(Ok(Token::ValueBool { value, .. })) => Ok(value),
            _ => Err(JsonDeserializerError::ParseError("Expected boolean".into())),
        }
    }

    fn read_byte(&mut self, _schema: &dyn Schema) -> Result<i8, Self::Error> {
        self.read_integer_value().and_then(|n| {
            i8::try_from(n).map_err(|_| {
                JsonDeserializerError::ParseError("Value out of range for byte".into())
            })
        })
    }

    fn read_short(&mut self, _schema: &dyn Schema) -> Result<i16, Self::Error> {
        self.read_integer_value().and_then(|n| {
            i16::try_from(n).map_err(|_| {
                JsonDeserializerError::ParseError("Value out of range for short".into())
            })
        })
    }

    fn read_integer(&mut self, _schema: &dyn Schema) -> Result<i32, Self::Error> {
        self.read_integer_value().and_then(|n| {
            i32::try_from(n).map_err(|_| {
                JsonDeserializerError::ParseError("Value out of range for integer".into())
            })
        })
    }

    fn read_long(&mut self, _schema: &dyn Schema) -> Result<i64, Self::Error> {
        self.read_integer_value()
    }

    fn read_float(&mut self, _schema: &dyn Schema) -> Result<f32, Self::Error> {
        self.read_float_value().map(|f| f as f32)
    }

    fn read_double(&mut self, _schema: &dyn Schema) -> Result<f64, Self::Error> {
        self.read_float_value()
    }

    fn read_big_integer(&mut self, _schema: &dyn Schema) -> Result<BigInteger, Self::Error> {
        use std::str::FromStr;
        let mut tokens = self.tokens();
        match tokens.next() {
            Some(Ok(Token::ValueNumber { .. })) => {
                // Extract string representation from input
                BigInteger::from_str("0")
                    .map_err(|e| JsonDeserializerError::ParseError(e.to_string()))
            }
            _ => Err(JsonDeserializerError::ParseError("Expected number".into())),
        }
    }

    fn read_big_decimal(&mut self, _schema: &dyn Schema) -> Result<BigDecimal, Self::Error> {
        use std::str::FromStr;
        let mut tokens = self.tokens();
        match tokens.next() {
            Some(Ok(Token::ValueNumber { .. })) => BigDecimal::from_str("0")
                .map_err(|e| JsonDeserializerError::ParseError(e.to_string())),
            _ => Err(JsonDeserializerError::ParseError("Expected number".into())),
        }
    }

    fn read_string(&mut self, _schema: &dyn Schema) -> Result<String, Self::Error> {
        let mut tokens = self.tokens();
        match tokens.next() {
            Some(Ok(Token::ValueString { value, .. })) => value
                .to_unescaped()
                .map(|s| s.into_owned())
                .map_err(|e| JsonDeserializerError::ParseError(e.to_string())),
            _ => Err(JsonDeserializerError::ParseError("Expected string".into())),
        }
    }

    fn read_blob(&mut self, _schema: &dyn Schema) -> Result<Blob, Self::Error> {
        let s = self.read_string(_schema)?;
        Ok(Blob::new(s.into_bytes()))
    }

    fn read_timestamp(&mut self, _schema: &dyn Schema) -> Result<DateTime, Self::Error> {
        let mut tokens = self.tokens();
        match tokens.next() {
            Some(Ok(Token::ValueNumber {
                value: Number::PosInt(n),
                ..
            })) => Ok(DateTime::from_secs(n as i64)),
            Some(Ok(Token::ValueNumber {
                value: Number::NegInt(n),
                ..
            })) => Ok(DateTime::from_secs(n)),
            _ => Err(JsonDeserializerError::ParseError(
                "Expected timestamp".into(),
            )),
        }
    }

    fn read_document(&mut self, _schema: &dyn Schema) -> Result<Document, Self::Error> {
        let mut tokens = self.tokens();
        match tokens.next() {
            Some(Ok(Token::ValueNull { .. })) => Ok(Document::Null),
            _ => Err(JsonDeserializerError::ParseError(
                "Document deserialization not fully implemented".into(),
            )),
        }
    }

    fn is_null(&self) -> bool {
        let mut tokens = self.tokens();
        matches!(tokens.next(), Some(Ok(Token::ValueNull { .. })))
    }

    fn container_size(&self) -> Option<usize> {
        None
    }
}

impl JsonDeserializer {
    fn read_integer_value(&mut self) -> Result<i64, JsonDeserializerError> {
        let mut tokens = self.tokens();
        match tokens.next() {
            Some(Ok(Token::ValueNumber {
                value: Number::PosInt(n),
                ..
            })) => i64::try_from(n)
                .map_err(|_| JsonDeserializerError::ParseError("Value out of range".into())),
            Some(Ok(Token::ValueNumber {
                value: Number::NegInt(n),
                ..
            })) => Ok(n),
            _ => Err(JsonDeserializerError::ParseError("Expected integer".into())),
        }
    }

    fn read_float_value(&mut self) -> Result<f64, JsonDeserializerError> {
        let mut tokens = self.tokens();
        match tokens.next() {
            Some(Ok(Token::ValueNumber {
                value: Number::Float(f),
                ..
            })) => Ok(f),
            Some(Ok(Token::ValueNumber {
                value: Number::PosInt(n),
                ..
            })) => Ok(n as f64),
            Some(Ok(Token::ValueNumber {
                value: Number::NegInt(n),
                ..
            })) => Ok(n as f64),
            _ => Err(JsonDeserializerError::ParseError("Expected number".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_schema() -> impl aws_smithy_schema::Schema {
        aws_smithy_schema::prelude::STRING
    }

    #[test]
    fn test_read_boolean() {
        let mut deser = JsonDeserializer::new(b"true", JsonCodecSettings::default());
        assert_eq!(deser.read_boolean(&dummy_schema()).unwrap(), true);

        let mut deser = JsonDeserializer::new(b"false", JsonCodecSettings::default());
        assert_eq!(deser.read_boolean(&dummy_schema()).unwrap(), false);
    }

    #[test]
    fn test_read_integer() {
        let mut deser = JsonDeserializer::new(b"42", JsonCodecSettings::default());
        assert_eq!(deser.read_integer(&dummy_schema()).unwrap(), 42);

        let mut deser = JsonDeserializer::new(b"-123", JsonCodecSettings::default());
        assert_eq!(deser.read_integer(&dummy_schema()).unwrap(), -123);
    }

    #[test]
    fn test_read_long() {
        let mut deser = JsonDeserializer::new(b"9223372036854775807", JsonCodecSettings::default());
        assert_eq!(deser.read_long(&dummy_schema()).unwrap(), i64::MAX);
    }

    #[test]
    fn test_read_float() {
        let mut deser = JsonDeserializer::new(b"3.14", JsonCodecSettings::default());
        assert!((deser.read_float(&dummy_schema()).unwrap() - 3.14).abs() < 0.01);
    }

    #[test]
    fn test_read_double() {
        let mut deser = JsonDeserializer::new(b"2.718", JsonCodecSettings::default());
        assert!((deser.read_double(&dummy_schema()).unwrap() - 2.718).abs() < 0.001);
    }

    #[test]
    fn test_read_string() {
        let mut deser = JsonDeserializer::new(br#""hello world""#, JsonCodecSettings::default());
        assert_eq!(deser.read_string(&dummy_schema()).unwrap(), "hello world");

        let mut deser = JsonDeserializer::new(br#""hello\nworld""#, JsonCodecSettings::default());
        assert_eq!(deser.read_string(&dummy_schema()).unwrap(), "hello\nworld");
    }

    #[test]
    fn test_is_null() {
        let deser = JsonDeserializer::new(b"null", JsonCodecSettings::default());
        assert!(deser.is_null());

        let deser = JsonDeserializer::new(b"42", JsonCodecSettings::default());
        assert!(!deser.is_null());
    }

    #[test]
    fn test_read_byte_range() {
        let mut deser = JsonDeserializer::new(b"127", JsonCodecSettings::default());
        assert_eq!(deser.read_byte(&dummy_schema()).unwrap(), 127);

        let mut deser = JsonDeserializer::new(b"128", JsonCodecSettings::default());
        assert!(deser.read_byte(&dummy_schema()).is_err());
    }
}
