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

    fn remaining(&self) -> &[u8] {
        &self.input[self.position..]
    }

    fn advance_by(&mut self, n: usize) {
        self.position += n;
    }
}

impl ShapeDeserializer for JsonDeserializer {
    type Error = JsonDeserializerError;

    fn read_struct<T, F>(
        &mut self,
        schema: &dyn Schema,
        mut state: T,
        mut consumer: F,
    ) -> Result<T, Self::Error>
    where
        F: FnMut(T, &dyn Schema, &mut Self) -> Result<T, Self::Error>,
    {
        // Expect opening brace
        self.skip_whitespace();
        if self.remaining().first() != Some(&b'{') {
            return Err(JsonDeserializerError::ParseError("Expected object".into()));
        }
        self.advance_by(1);

        loop {
            self.skip_whitespace();

            // Check for end of object
            if self.remaining().first() == Some(&b'}') {
                self.advance_by(1);
                break;
            }

            // Expect a key (quoted string)
            if self.remaining().first() != Some(&b'"') {
                return Err(JsonDeserializerError::ParseError(
                    "Expected object key".into(),
                ));
            }

            // Parse the key using the token iterator
            let mut iter = json_token_iter(self.remaining());
            let key_str = match iter.next() {
                Some(Ok(Token::ValueString { value, .. })) => {
                    let key_len = value.as_escaped_str().len();
                    let key = value
                        .to_unescaped()
                        .map_err(|e| JsonDeserializerError::ParseError(e.to_string()))?
                        .into_owned();
                    // Advance past opening quote + key + closing quote
                    self.advance_by(key_len + 2);
                    key
                }
                _ => {
                    return Err(JsonDeserializerError::ParseError(
                        "Expected object key".into(),
                    ))
                }
            };

            // Skip whitespace and expect colon
            self.skip_whitespace();
            if self.remaining().first() != Some(&b':') {
                return Err(JsonDeserializerError::ParseError(
                    "Expected colon after key".into(),
                ));
            }
            self.advance_by(1);
            self.skip_whitespace();

            // Process the value
            if let Some(member_schema) = schema.member_schema(&key_str) {
                state = consumer(state, member_schema, self)?;
            } else {
                self.skip_value()?;
            }
        }

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
        let mut iter = json_token_iter(self.remaining());
        match iter.next() {
            Some(Ok(Token::ValueBool { value, .. })) => {
                self.advance_by(if value { 4 } else { 5 });
                Ok(value)
            }
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
        let mut iter = json_token_iter(self.remaining());
        match iter.next() {
            Some(Ok(Token::ValueNumber { .. })) => {
                self.consume_number();
                BigInteger::from_str("0")
                    .map_err(|e| JsonDeserializerError::ParseError(e.to_string()))
            }
            _ => Err(JsonDeserializerError::ParseError("Expected number".into())),
        }
    }

    fn read_big_decimal(&mut self, _schema: &dyn Schema) -> Result<BigDecimal, Self::Error> {
        use std::str::FromStr;
        let mut iter = json_token_iter(self.remaining());
        match iter.next() {
            Some(Ok(Token::ValueNumber { .. })) => {
                self.consume_number();
                BigDecimal::from_str("0")
                    .map_err(|e| JsonDeserializerError::ParseError(e.to_string()))
            }
            _ => Err(JsonDeserializerError::ParseError("Expected number".into())),
        }
    }

    fn read_string(&mut self, _schema: &dyn Schema) -> Result<String, Self::Error> {
        let mut iter = json_token_iter(self.remaining());
        match iter.next() {
            Some(Ok(Token::ValueString { value, .. })) => {
                let len = value.as_escaped_str().len();
                let result = value
                    .to_unescaped()
                    .map(|s| s.into_owned())
                    .map_err(|e| JsonDeserializerError::ParseError(e.to_string()))?;
                self.advance_by(len + 2);
                Ok(result)
            }
            _ => Err(JsonDeserializerError::ParseError("Expected string".into())),
        }
    }

    fn read_blob(&mut self, _schema: &dyn Schema) -> Result<Blob, Self::Error> {
        let s = self.read_string(_schema)?;
        Ok(Blob::new(s.into_bytes()))
    }

    fn read_timestamp(&mut self, _schema: &dyn Schema) -> Result<DateTime, Self::Error> {
        let mut iter = json_token_iter(self.remaining());
        match iter.next() {
            Some(Ok(Token::ValueNumber {
                value: Number::PosInt(n),
                ..
            })) => {
                self.consume_number();
                Ok(DateTime::from_secs(n as i64))
            }
            Some(Ok(Token::ValueNumber {
                value: Number::NegInt(n),
                ..
            })) => {
                self.consume_number();
                Ok(DateTime::from_secs(n))
            }
            _ => Err(JsonDeserializerError::ParseError(
                "Expected timestamp".into(),
            )),
        }
    }

    fn read_document(&mut self, _schema: &dyn Schema) -> Result<Document, Self::Error> {
        let mut iter = json_token_iter(self.remaining());
        match iter.next() {
            Some(Ok(Token::ValueNull { .. })) => {
                self.advance_by(4);
                Ok(Document::Null)
            }
            _ => Err(JsonDeserializerError::ParseError(
                "Document deserialization not fully implemented".into(),
            )),
        }
    }

    fn is_null(&self) -> bool {
        let mut iter = json_token_iter(self.remaining());
        matches!(iter.next(), Some(Ok(Token::ValueNull { .. })))
    }

    fn container_size(&self) -> Option<usize> {
        None
    }
}

impl JsonDeserializer {
    fn skip_whitespace(&mut self) {
        while self.position < self.input.len() {
            match self.input[self.position] {
                b' ' | b'\t' | b'\n' | b'\r' | b',' => self.position += 1,
                _ => break,
            }
        }
    }

    fn consume_number(&mut self) {
        let mut len = 0;
        for &b in self.remaining() {
            if b.is_ascii_digit() || b == b'-' || b == b'.' || b == b'e' || b == b'E' || b == b'+' {
                len += 1;
            } else {
                break;
            }
        }
        self.advance_by(len);
    }

    fn skip_value(&mut self) -> Result<(), JsonDeserializerError> {
        let mut depth = 0;
        loop {
            let mut iter = json_token_iter(self.remaining());
            match iter.next() {
                Some(Ok(Token::StartObject { .. })) | Some(Ok(Token::StartArray { .. })) => {
                    self.advance_by(1);
                    depth += 1;
                }
                Some(Ok(Token::EndObject { .. })) | Some(Ok(Token::EndArray { .. })) => {
                    self.advance_by(1);
                    if depth == 0 {
                        return Err(JsonDeserializerError::ParseError(
                            "Unexpected end token".into(),
                        ));
                    }
                    depth -= 1;
                    if depth == 0 {
                        return Ok(());
                    }
                }
                Some(Ok(Token::ValueBool { value, .. })) if depth == 0 => {
                    self.advance_by(if value { 4 } else { 5 });
                    return Ok(());
                }
                Some(Ok(Token::ValueNull { .. })) if depth == 0 => {
                    self.advance_by(4);
                    return Ok(());
                }
                Some(Ok(Token::ValueString { value, .. })) if depth == 0 => {
                    self.advance_by(value.as_escaped_str().len() + 2);
                    return Ok(());
                }
                Some(Ok(Token::ValueNumber { .. })) if depth == 0 => {
                    self.consume_number();
                    return Ok(());
                }
                Some(Ok(Token::ObjectKey { key, .. })) => {
                    self.advance_by(key.as_escaped_str().len() + 3);
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(JsonDeserializerError::ParseError(e.to_string())),
                None => {
                    return Err(JsonDeserializerError::ParseError(
                        "Unexpected end of input".into(),
                    ))
                }
            }
        }
    }

    fn read_integer_value(&mut self) -> Result<i64, JsonDeserializerError> {
        let mut iter = json_token_iter(self.remaining());
        match iter.next() {
            Some(Ok(Token::ValueNumber {
                value: Number::PosInt(n),
                ..
            })) => {
                self.consume_number();
                i64::try_from(n)
                    .map_err(|_| JsonDeserializerError::ParseError("Value out of range".into()))
            }
            Some(Ok(Token::ValueNumber {
                value: Number::NegInt(n),
                ..
            })) => {
                self.consume_number();
                Ok(n)
            }
            _ => Err(JsonDeserializerError::ParseError("Expected integer".into())),
        }
    }

    fn read_float_value(&mut self) -> Result<f64, JsonDeserializerError> {
        let mut iter = json_token_iter(self.remaining());
        match iter.next() {
            Some(Ok(Token::ValueNumber {
                value: Number::Float(f),
                ..
            })) => {
                self.consume_number();
                Ok(f)
            }
            Some(Ok(Token::ValueNumber {
                value: Number::PosInt(n),
                ..
            })) => {
                self.consume_number();
                Ok(n as f64)
            }
            Some(Ok(Token::ValueNumber {
                value: Number::NegInt(n),
                ..
            })) => {
                self.consume_number();
                Ok(n as f64)
            }
            _ => Err(JsonDeserializerError::ParseError("Expected number".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_schema() -> &'static impl aws_smithy_schema::Schema {
        &aws_smithy_schema::prelude::STRING
    }

    #[test]
    fn test_read_boolean() {
        let mut deser = JsonDeserializer::new(b"true", JsonCodecSettings::default());
        assert_eq!(deser.read_boolean(dummy_schema()).unwrap(), true);

        let mut deser = JsonDeserializer::new(b"false", JsonCodecSettings::default());
        assert_eq!(deser.read_boolean(dummy_schema()).unwrap(), false);
    }

    #[test]
    fn test_read_integer() {
        let mut deser = JsonDeserializer::new(b"42", JsonCodecSettings::default());
        assert_eq!(deser.read_integer(dummy_schema()).unwrap(), 42);

        let mut deser = JsonDeserializer::new(b"-123", JsonCodecSettings::default());
        assert_eq!(deser.read_integer(dummy_schema()).unwrap(), -123);
    }

    #[test]
    fn test_read_long() {
        let mut deser = JsonDeserializer::new(b"9223372036854775807", JsonCodecSettings::default());
        assert_eq!(deser.read_long(dummy_schema()).unwrap(), i64::MAX);
    }

    #[test]
    fn test_read_float() {
        let mut deser = JsonDeserializer::new(b"3.14", JsonCodecSettings::default());
        assert!((deser.read_float(dummy_schema()).unwrap() - 3.14).abs() < 0.01);
    }

    #[test]
    fn test_read_double() {
        let mut deser = JsonDeserializer::new(b"2.718", JsonCodecSettings::default());
        assert!((deser.read_double(dummy_schema()).unwrap() - 2.718).abs() < 0.001);
    }

    #[test]
    fn test_read_string() {
        let mut deser = JsonDeserializer::new(br#""hello world""#, JsonCodecSettings::default());
        assert_eq!(deser.read_string(dummy_schema()).unwrap(), "hello world");

        let mut deser = JsonDeserializer::new(br#""hello\nworld""#, JsonCodecSettings::default());
        assert_eq!(deser.read_string(dummy_schema()).unwrap(), "hello\nworld");
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
        assert_eq!(deser.read_byte(dummy_schema()).unwrap(), 127);

        let mut deser = JsonDeserializer::new(b"128", JsonCodecSettings::default());
        assert!(deser.read_byte(dummy_schema()).is_err());
    }

    #[test]
    fn test_read_struct() {
        use aws_smithy_schema::{prelude, Schema, ShapeId, ShapeType, TraitMap};

        #[derive(Debug, Default, PartialEq)]
        struct Person {
            first_name: String,
            last_name: String,
            age: i32,
        }

        struct MemberSchema {
            name: &'static str,
            target: &'static dyn Schema,
        }

        impl Schema for MemberSchema {
            fn shape_id(&self) -> &ShapeId {
                self.target.shape_id()
            }
            fn shape_type(&self) -> ShapeType {
                self.target.shape_type()
            }
            fn traits(&self) -> &TraitMap {
                self.target.traits()
            }
            fn member_name(&self) -> Option<&str> {
                Some(self.name)
            }
        }

        static FIRST_NAME: MemberSchema = MemberSchema {
            name: "firstName",
            target: &prelude::STRING,
        };
        static LAST_NAME: MemberSchema = MemberSchema {
            name: "lastName",
            target: &prelude::STRING,
        };
        static AGE: MemberSchema = MemberSchema {
            name: "age",
            target: &prelude::INTEGER,
        };

        struct TestSchema;
        impl Schema for TestSchema {
            fn shape_id(&self) -> &ShapeId {
                static ID: std::sync::LazyLock<ShapeId> =
                    std::sync::LazyLock::new(|| ShapeId::from_static("test#Test", "test", "Test"));
                &ID
            }
            fn shape_type(&self) -> ShapeType {
                ShapeType::Structure
            }
            fn traits(&self) -> &TraitMap {
                static MAP: std::sync::LazyLock<TraitMap> = std::sync::LazyLock::new(TraitMap::new);
                &MAP
            }
            fn member_schema(&self, name: &str) -> Option<&dyn Schema> {
                match name {
                    "firstName" => Some(&FIRST_NAME),
                    "lastName" => Some(&LAST_NAME),
                    "age" => Some(&AGE),
                    _ => None,
                }
            }
            fn member_schema_by_index(&self, index: usize) -> Option<(&str, &dyn Schema)> {
                match index {
                    0 => Some(("firstName", &FIRST_NAME)),
                    1 => Some(("lastName", &LAST_NAME)),
                    2 => Some(("age", &AGE)),
                    _ => None,
                }
            }
        }

        fn consume_person(
            mut person: Person,
            schema: &dyn Schema,
            deser: &mut JsonDeserializer,
        ) -> Result<Person, JsonDeserializerError> {
            match schema.member_name() {
                Some("firstName") => person.first_name = deser.read_string(schema)?,
                Some("lastName") => person.last_name = deser.read_string(schema)?,
                Some("age") => person.age = deser.read_integer(schema)?,
                _ => {}
            }
            Ok(person)
        }

        let json = br#"{"lastName":"Smithy","firstName":"Alice","age":30}"#;
        let mut deser = JsonDeserializer::new(json, JsonCodecSettings::default());
        let person = deser
            .read_struct(&TestSchema, Person::default(), consume_person)
            .unwrap();
        assert_eq!(
            person,
            Person {
                first_name: "Alice".to_string(),
                last_name: "Smithy".to_string(),
                age: 30
            }
        );

        let json =
            br#"{"firstName":          "Alice","age":12345678,     "lastName":"\"Smithy\""}"#;
        let mut deser = JsonDeserializer::new(json, JsonCodecSettings::default());
        let person = deser
            .read_struct(&TestSchema, Person::default(), consume_person)
            .unwrap();
        assert_eq!(
            person,
            Person {
                first_name: "Alice".to_string(),
                last_name: "\"Smithy\"".to_string(),
                age: 12345678
            }
        );
    }
}
