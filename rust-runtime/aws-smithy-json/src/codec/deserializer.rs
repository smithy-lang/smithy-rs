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
pub struct JsonDeserializer<'a> {
    input: &'a [u8],
    position: usize,
    // TODO(schema): Need to figure out how this will work with traits like
    // jsonName. Will figure that out once I am codegening real Schemas
    _settings: JsonCodecSettings,
}

impl<'a> JsonDeserializer<'a> {
    /// Creates a new JSON deserializer with the given settings.
    pub fn new(input: &'a [u8], settings: JsonCodecSettings) -> Self {
        Self {
            input,
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

impl<'a> ShapeDeserializer for JsonDeserializer<'a> {
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
        mut state: T,
        mut consumer: F,
    ) -> Result<T, Self::Error>
    where
        F: FnMut(T, &mut Self) -> Result<T, Self::Error>,
    {
        self.skip_whitespace();
        if self.remaining().first() != Some(&b'[') {
            return Err(JsonDeserializerError::ParseError("Expected array".into()));
        }
        self.advance_by(1);

        loop {
            self.skip_whitespace();
            if self.remaining().first() == Some(&b']') {
                self.advance_by(1);
                break;
            }
            state = consumer(state, self)?;
        }

        Ok(state)
    }

    fn read_map<T, F>(
        &mut self,
        _schema: &dyn Schema,
        mut state: T,
        mut consumer: F,
    ) -> Result<T, Self::Error>
    where
        F: FnMut(T, String, &mut Self) -> Result<T, Self::Error>,
    {
        self.skip_whitespace();
        if self.remaining().first() != Some(&b'{') {
            return Err(JsonDeserializerError::ParseError("Expected object".into()));
        }
        self.advance_by(1);

        loop {
            self.skip_whitespace();
            if self.remaining().first() == Some(&b'}') {
                self.advance_by(1);
                break;
            }

            if self.remaining().first() != Some(&b'"') {
                return Err(JsonDeserializerError::ParseError("Expected key".into()));
            }

            let mut iter = json_token_iter(self.remaining());
            let key = match iter.next() {
                Some(Ok(Token::ValueString { value, .. })) => {
                    let len = value.as_escaped_str().len();
                    let key = value
                        .to_unescaped()
                        .map_err(|e| JsonDeserializerError::ParseError(e.to_string()))?
                        .into_owned();
                    self.advance_by(len + 2);
                    key
                }
                _ => return Err(JsonDeserializerError::ParseError("Expected key".into())),
            };

            self.skip_whitespace();
            if self.remaining().first() != Some(&b':') {
                return Err(JsonDeserializerError::ParseError("Expected colon".into()));
            }
            self.advance_by(1);
            self.skip_whitespace();

            state = consumer(state, key, self)?;
        }

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
        let mut iter = json_token_iter(self.remaining());
        match iter.next()? {
            Ok(Token::StartArray { .. }) => {
                let mut count = 0;
                let mut depth = 1;
                for token in iter {
                    match token {
                        Ok(Token::StartArray { .. }) | Ok(Token::StartObject { .. }) => {
                            if depth == 1 {
                                count += 1;
                            }
                            depth += 1;
                        }
                        Ok(Token::EndArray { .. }) | Ok(Token::EndObject { .. }) => {
                            depth -= 1;
                            if depth == 0 {
                                return Some(count);
                            }
                        }
                        Ok(Token::ValueBool { .. })
                        | Ok(Token::ValueNull { .. })
                        | Ok(Token::ValueString { .. })
                        | Ok(Token::ValueNumber { .. })
                            if depth == 1 =>
                        {
                            count += 1
                        }
                        _ => {}
                    }
                }
                None
            }
            Ok(Token::StartObject { .. }) => {
                let mut count = 0;
                let mut depth = 1;
                for token in iter {
                    match token {
                        Ok(Token::StartArray { .. }) | Ok(Token::StartObject { .. }) => depth += 1,
                        Ok(Token::EndArray { .. }) | Ok(Token::EndObject { .. }) => {
                            depth -= 1;
                            if depth == 0 {
                                return Some(count);
                            }
                        }
                        Ok(Token::ObjectKey { .. }) if depth == 1 => count += 1,
                        _ => {}
                    }
                }
                None
            }
            _ => None,
        }
    }
}

impl<'a> JsonDeserializer<'a> {
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

    #[test]
    fn test_read_list() {
        let json = b"[1, 2, 3, 4, 5]";
        let mut deser = JsonDeserializer::new(json, JsonCodecSettings::default());
        let capacity = deser.container_size().unwrap_or(0);
        let container = Vec::with_capacity(capacity);
        let allocated_capacity = container.capacity();
        let result = deser
            .read_list(dummy_schema(), container, |mut vec, deser| {
                vec.push(deser.read_integer(dummy_schema())?);
                Ok(vec)
            })
            .unwrap();
        assert_eq!(result, vec![1, 2, 3, 4, 5]);
        // Ensure no more memory was allocated for the container
        assert_eq!(result.capacity(), allocated_capacity);

        let json = b"[]";
        let mut deser = JsonDeserializer::new(json, JsonCodecSettings::default());
        let capacity = deser.container_size().unwrap_or(0);
        let container = Vec::with_capacity(capacity);
        let allocated_capacity = container.capacity();
        let result = deser
            .read_list(dummy_schema(), container, |mut vec, deser| {
                vec.push(deser.read_integer(dummy_schema())?);
                Ok(vec)
            })
            .unwrap();
        assert_eq!(result, Vec::<i32>::new());
        // Ensure no more memory was allocated for the container
        assert_eq!(result.capacity(), allocated_capacity);

        let json = br#"["hello", "world"]"#;
        let mut deser = JsonDeserializer::new(json, JsonCodecSettings::default());
        let capacity = deser.container_size().unwrap_or(0);
        let container = Vec::with_capacity(capacity);
        let allocated_capacity = container.capacity();
        let result = deser
            .read_list(dummy_schema(), container, |mut vec, deser| {
                vec.push(deser.read_string(dummy_schema())?);
                Ok(vec)
            })
            .unwrap();
        assert_eq!(result, vec!["hello", "world"]);
        // Ensure no more memory was allocated for the container
        assert_eq!(result.capacity(), allocated_capacity);
    }

    #[test]
    fn test_container_size() {
        let deser = JsonDeserializer::new(b"[1, 2, 3, 4, 5]", JsonCodecSettings::default());
        assert_eq!(deser.container_size(), Some(5));

        let deser = JsonDeserializer::new(b"[]", JsonCodecSettings::default());
        assert_eq!(deser.container_size(), Some(0));

        let deser =
            JsonDeserializer::new(br#"{"a": 1, "b": 2, "c": 3}"#, JsonCodecSettings::default());
        assert_eq!(deser.container_size(), Some(3));

        let deser = JsonDeserializer::new(b"{}", JsonCodecSettings::default());
        assert_eq!(deser.container_size(), Some(0));

        let deser =
            JsonDeserializer::new(b"[[1, 2], [3, 4], [5, 6]]", JsonCodecSettings::default());
        assert_eq!(deser.container_size(), Some(3));

        let deser = JsonDeserializer::new(b"42", JsonCodecSettings::default());
        assert_eq!(deser.container_size(), None);
    }

    #[test]
    fn test_read_map() {
        use std::collections::HashMap;

        let json = br#"{"a": 1, "b": 2, "c": 3}"#;
        let mut deser = JsonDeserializer::new(json, JsonCodecSettings::default());
        let calculated_capacity = deser.container_size().unwrap_or(0);
        let container = HashMap::with_capacity(calculated_capacity);
        let allocated_capacity = container.capacity();
        let result = deser
            .read_map(dummy_schema(), container, |mut map, key, deser| {
                map.insert(key, deser.read_integer(dummy_schema())?);
                Ok(map)
            })
            .unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result.get("a"), Some(&1));
        assert_eq!(result.get("b"), Some(&2));
        assert_eq!(result.get("c"), Some(&3));
        // Ensure no more memory was allocated for the container
        assert_eq!(result.capacity(), allocated_capacity);

        let json = b"{}";
        let mut deser = JsonDeserializer::new(json, JsonCodecSettings::default());
        let calculated_capacity = deser.container_size().unwrap_or(0);
        let container = HashMap::with_capacity(calculated_capacity);
        let allocated_capacity = container.capacity();
        let result = deser
            .read_map(dummy_schema(), container, |mut map, key, deser| {
                map.insert(key, deser.read_integer(dummy_schema())?);
                Ok(map)
            })
            .unwrap();
        assert_eq!(result, HashMap::<String, i32>::new());
        // Ensure no more memory was allocated for the container
        assert_eq!(result.capacity(), allocated_capacity);

        let json = br#"{"name": "Alice", "city": "Seattle"}"#;
        let mut deser = JsonDeserializer::new(json, JsonCodecSettings::default());
        let calculated_capacity = deser.container_size().unwrap_or(0);
        let container = HashMap::with_capacity(calculated_capacity);
        let allocated_capacity = container.capacity();
        let result = deser
            .read_map(dummy_schema(), container, |mut map, key, deser| {
                map.insert(key, deser.read_string(dummy_schema())?);
                Ok(map)
            })
            .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result.get("name"), Some(&"Alice".to_string()));
        assert_eq!(result.get("city"), Some(&"Seattle".to_string()));
        // Ensure no more memory was allocated for the container
        assert_eq!(result.capacity(), allocated_capacity);
    }

    #[test]
    fn test_nested_complex_deserialization() {
        use aws_smithy_schema::{prelude, Schema, ShapeId, ShapeType, TraitMap};
        use std::collections::HashMap;

        #[derive(Debug, Default, PartialEq)]
        struct Address {
            street: String,
            city: String,
            zip: i32,
        }

        #[derive(Debug, Default, PartialEq)]
        struct Company {
            name: String,
            employees: Vec<String>,
            metadata: HashMap<String, i32>,
            active: bool,
        }

        #[derive(Debug, Default, PartialEq)]
        struct User {
            id: i64,
            name: String,
            scores: Vec<f64>,
            address: Address,
            companies: Vec<Company>,
            tags: HashMap<String, String>,
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

        static ADDR_STREET: MemberSchema = MemberSchema {
            name: "street",
            target: &prelude::STRING,
        };
        static ADDR_CITY: MemberSchema = MemberSchema {
            name: "city",
            target: &prelude::STRING,
        };
        static ADDR_ZIP: MemberSchema = MemberSchema {
            name: "zip",
            target: &prelude::INTEGER,
        };

        struct AddressSchema;
        impl Schema for AddressSchema {
            fn shape_id(&self) -> &ShapeId {
                static ID: std::sync::LazyLock<ShapeId> = std::sync::LazyLock::new(|| {
                    ShapeId::from_static("test#Address", "test", "Address")
                });
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
                    "street" => Some(&ADDR_STREET),
                    "city" => Some(&ADDR_CITY),
                    "zip" => Some(&ADDR_ZIP),
                    _ => None,
                }
            }
            fn member_schema_by_index(&self, index: usize) -> Option<(&str, &dyn Schema)> {
                match index {
                    0 => Some(("street", &ADDR_STREET)),
                    1 => Some(("city", &ADDR_CITY)),
                    2 => Some(("zip", &ADDR_ZIP)),
                    _ => None,
                }
            }
        }

        static COMP_NAME: MemberSchema = MemberSchema {
            name: "name",
            target: &prelude::STRING,
        };
        static COMP_ACTIVE: MemberSchema = MemberSchema {
            name: "active",
            target: &prelude::BOOLEAN,
        };
        static COMP_EMPLOYEES: MemberSchema = MemberSchema {
            name: "employees",
            target: &prelude::STRING,
        };
        static COMP_METADATA: MemberSchema = MemberSchema {
            name: "metadata",
            target: &prelude::STRING,
        };

        struct CompanySchema;
        impl Schema for CompanySchema {
            fn shape_id(&self) -> &ShapeId {
                static ID: std::sync::LazyLock<ShapeId> = std::sync::LazyLock::new(|| {
                    ShapeId::from_static("test#Company", "test", "Company")
                });
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
                    "name" => Some(&COMP_NAME),
                    "active" => Some(&COMP_ACTIVE),
                    "employees" => Some(&COMP_EMPLOYEES),
                    "metadata" => Some(&COMP_METADATA),
                    _ => None,
                }
            }
            fn member_schema_by_index(&self, index: usize) -> Option<(&str, &dyn Schema)> {
                match index {
                    0 => Some(("name", &COMP_NAME)),
                    1 => Some(("active", &COMP_ACTIVE)),
                    _ => None,
                }
            }
        }

        static USER_ID: MemberSchema = MemberSchema {
            name: "id",
            target: &prelude::LONG,
        };
        static USER_NAME: MemberSchema = MemberSchema {
            name: "name",
            target: &prelude::STRING,
        };
        static USER_SCORES: MemberSchema = MemberSchema {
            name: "scores",
            target: &prelude::STRING,
        };
        static USER_ADDRESS: MemberSchema = MemberSchema {
            name: "address",
            target: &prelude::STRING,
        };
        static USER_COMPANIES: MemberSchema = MemberSchema {
            name: "companies",
            target: &prelude::STRING,
        };
        static USER_TAGS: MemberSchema = MemberSchema {
            name: "tags",
            target: &prelude::STRING,
        };

        struct UserSchema;
        impl Schema for UserSchema {
            fn shape_id(&self) -> &ShapeId {
                static ID: std::sync::LazyLock<ShapeId> =
                    std::sync::LazyLock::new(|| ShapeId::from_static("test#User", "test", "User"));
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
                    "id" => Some(&USER_ID),
                    "name" => Some(&USER_NAME),
                    "scores" => Some(&USER_SCORES),
                    "address" => Some(&USER_ADDRESS),
                    "companies" => Some(&USER_COMPANIES),
                    "tags" => Some(&USER_TAGS),
                    _ => None,
                }
            }
            fn member_schema_by_index(&self, index: usize) -> Option<(&str, &dyn Schema)> {
                match index {
                    0 => Some(("id", &USER_ID)),
                    1 => Some(("name", &USER_NAME)),
                    _ => None,
                }
            }
        }

        fn consume_address(
            mut addr: Address,
            schema: &dyn Schema,
            deser: &mut JsonDeserializer,
        ) -> Result<Address, JsonDeserializerError> {
            match schema.member_name() {
                Some("street") => addr.street = deser.read_string(schema)?,
                Some("city") => addr.city = deser.read_string(schema)?,
                Some("zip") => addr.zip = deser.read_integer(schema)?,
                _ => {}
            }
            Ok(addr)
        }

        fn consume_company(
            mut comp: Company,
            schema: &dyn Schema,
            deser: &mut JsonDeserializer,
        ) -> Result<Company, JsonDeserializerError> {
            match schema.member_name() {
                Some("name") => comp.name = deser.read_string(schema)?,
                Some("active") => comp.active = deser.read_boolean(schema)?,
                Some("employees") => {
                    comp.employees = deser.read_list(schema, Vec::new(), |mut v, d| {
                        v.push(d.read_string(dummy_schema())?);
                        Ok(v)
                    })?
                }
                Some("metadata") => {
                    comp.metadata = deser.read_map(schema, HashMap::new(), |mut m, k, d| {
                        m.insert(k, d.read_integer(dummy_schema())?);
                        Ok(m)
                    })?
                }
                _ => {}
            }
            Ok(comp)
        }

        fn consume_user(
            mut user: User,
            schema: &dyn Schema,
            deser: &mut JsonDeserializer,
        ) -> Result<User, JsonDeserializerError> {
            match schema.member_name() {
                Some("id") => user.id = deser.read_long(schema)?,
                Some("name") => user.name = deser.read_string(schema)?,
                Some("scores") => {
                    user.scores = deser.read_list(schema, Vec::new(), |mut v, d| {
                        v.push(d.read_double(dummy_schema())?);
                        Ok(v)
                    })?
                }
                Some("address") => {
                    user.address =
                        deser.read_struct(&AddressSchema, Address::default(), consume_address)?
                }
                Some("companies") => {
                    user.companies = deser.read_list(schema, Vec::new(), |mut v, d| {
                        v.push(d.read_struct(
                            &CompanySchema,
                            Company::default(),
                            consume_company,
                        )?);
                        Ok(v)
                    })?
                }
                Some("tags") => {
                    user.tags = deser.read_map(schema, HashMap::new(), |mut m, k, d| {
                        m.insert(k, d.read_string(dummy_schema())?);
                        Ok(m)
                    })?
                }
                _ => {}
            }
            Ok(user)
        }

        let json = br#"{
            "id": 12345,
            "name": "John Doe",
            "scores": [95.5, 87.3, 92.1],
            "address": {
                "street": "123 Main St",
                "city": "Seattle",
                "zip": 98101
            },
            "companies": [
                {
                    "name": "TechCorp",
                    "employees": ["Alice", "Bob"],
                    "metadata": {"founded": 2010, "size": 500},
                    "active": true
                },
                {
                    "name": "StartupInc",
                    "employees": ["Charlie"],
                    "metadata": {"founded": 2020},
                    "active": false
                }
            ],
            "tags": {"role": "admin", "level": "senior"}
        }"#;

        let mut deser = JsonDeserializer::new(json, JsonCodecSettings::default());
        let user = deser
            .read_struct(&UserSchema, User::default(), consume_user)
            .unwrap();

        assert_eq!(user.id, 12345);
        assert_eq!(user.name, "John Doe");
        assert_eq!(user.scores, vec![95.5, 87.3, 92.1]);
        assert_eq!(user.address.street, "123 Main St");
        assert_eq!(user.address.city, "Seattle");
        assert_eq!(user.address.zip, 98101);
        assert_eq!(user.companies.len(), 2);
        assert_eq!(user.companies[0].name, "TechCorp");
        assert_eq!(user.companies[0].employees, vec!["Alice", "Bob"]);
        assert_eq!(user.companies[0].metadata.get("founded"), Some(&2010));
        assert_eq!(user.companies[0].metadata.get("size"), Some(&500));
        assert_eq!(user.companies[0].active, true);
        assert_eq!(user.companies[1].name, "StartupInc");
        assert_eq!(user.companies[1].employees, vec!["Charlie"]);
        assert_eq!(user.companies[1].metadata.get("founded"), Some(&2020));
        assert_eq!(user.companies[1].active, false);
        assert_eq!(user.tags.get("role"), Some(&"admin".to_string()));
        assert_eq!(user.tags.get("level"), Some(&"senior".to_string()));
    }
}
