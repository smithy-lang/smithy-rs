/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! JSON deserializer implementation.

use aws_smithy_schema::serde::ShapeDeserializer;
use aws_smithy_schema::Schema;
use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Document};
use std::fmt;

use crate::codec::JsonCodecSettings;

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
    _input: Vec<u8>,
    _settings: JsonCodecSettings,
}

impl JsonDeserializer {
    /// Creates a new JSON deserializer with the given settings.
    pub fn new(input: &[u8], settings: JsonCodecSettings) -> Self {
        Self {
            _input: input.to_vec(),
            _settings: settings,
        }
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

    fn read_big_integer(&mut self, _schema: &dyn Schema) -> Result<BigInteger, Self::Error> {
        use std::str::FromStr;
        Ok(BigInteger::from_str("0").unwrap())
    }

    fn read_big_decimal(&mut self, _schema: &dyn Schema) -> Result<BigDecimal, Self::Error> {
        use std::str::FromStr;
        Ok(BigDecimal::from_str("0").unwrap())
    }

    fn read_string(&mut self, _schema: &dyn Schema) -> Result<String, Self::Error> {
        Ok(String::new())
    }

    fn read_blob(&mut self, _schema: &dyn Schema) -> Result<Blob, Self::Error> {
        Ok(Blob::new(Vec::new()))
    }

    fn read_timestamp(&mut self, _schema: &dyn Schema) -> Result<DateTime, Self::Error> {
        Ok(DateTime::from_secs(0))
    }

    fn read_document(&mut self, _schema: &dyn Schema) -> Result<Document, Self::Error> {
        Ok(Document::Null)
    }

    fn is_null(&self) -> bool {
        false
    }

    fn container_size(&self) -> Option<usize> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserializer_creation() {
        let _deser = JsonDeserializer::new(b"{}", JsonCodecSettings::default());
    }
}
