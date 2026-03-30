/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! JSON deserializer implementation.

use aws_smithy_schema::serde::SerdeError;
use aws_smithy_schema::serde::ShapeDeserializer;
use aws_smithy_schema::Schema;
use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Document, Number};

use crate::codec::JsonCodecSettings;
use crate::deserialize::{json_token_iter, Token};

use std::sync::Arc;

/// JSON deserializer that implements the ShapeDeserializer trait.
pub struct JsonDeserializer<'a> {
    input: &'a [u8],
    position: usize,
    settings: Arc<JsonCodecSettings>,
}

impl<'a> JsonDeserializer<'a> {
    /// Creates a new JSON deserializer with the given settings.
    pub(crate) fn new(input: &'a [u8], settings: Arc<JsonCodecSettings>) -> Self {
        Self {
            input,
            position: 0,
            settings,
        }
    }

    /// Resolves a JSON field name to a member schema.
    fn resolve_member<'s>(&self, schema: &'s Schema, field_name: &str) -> Option<&'s Schema> {
        self.settings.field_to_member(schema, field_name)
    }

    fn remaining(&self) -> &[u8] {
        &self.input[self.position..]
    }

    fn advance_by(&mut self, n: usize) {
        self.position += n;
    }

    /// Parse a JSON quoted string key directly from bytes, advancing past it.
    /// Assumes the current position is at the opening `"`.
    fn parse_key(&mut self) -> Result<String, SerdeError> {
        self.advance_by(1); // skip opening quote
        let remaining = self.remaining();
        let mut i = 0;
        let mut has_escapes = false;
        while i < remaining.len() {
            match remaining[i] {
                b'"' => break,
                b'\\' => {
                    has_escapes = true;
                    i += 2;
                }
                _ => i += 1,
            }
        }
        let key = if has_escapes {
            let raw =
                std::str::from_utf8(&remaining[..i]).map_err(|e| SerdeError::InvalidInput {
                    message: e.to_string(),
                })?;
            crate::escape::unescape_string(raw)
                .map_err(|e| SerdeError::InvalidInput {
                    message: e.to_string(),
                })?
                .into_owned()
        } else {
            std::str::from_utf8(&remaining[..i])
                .map_err(|e| SerdeError::InvalidInput {
                    message: e.to_string(),
                })?
                .to_owned()
        };
        self.advance_by(i + 1); // key bytes + closing quote
        Ok(key)
    }
}

impl<'a> ShapeDeserializer for JsonDeserializer<'a> {
    fn read_struct(
        &mut self,
        schema: &Schema,
        consumer: &mut dyn FnMut(&Schema, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        // Expect opening brace
        self.skip_whitespace();
        if self.remaining().first() != Some(&b'{') {
            return Err(SerdeError::TypeMismatch {
                message: "expected object".into(),
            });
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
                return Err(SerdeError::InvalidInput {
                    message: "expected object key".into(),
                });
            }

            // Parse the key directly from bytes
            let key_str = self.parse_key()?;

            // Skip whitespace and expect colon
            self.skip_whitespace();
            if self.remaining().first() != Some(&b':') {
                return Err(SerdeError::InvalidInput {
                    message: "expected colon after key".into(),
                });
            }
            self.advance_by(1);
            self.skip_whitespace();

            // Process the value — skip nulls (they represent absent optional members)
            let rem = self.remaining();
            if rem.starts_with(b"null") && !rem.get(4).is_some_and(|b| b.is_ascii_alphanumeric()) {
                self.advance_by(4);
            } else if let Some(member_schema) = self.resolve_member(schema, &key_str) {
                consumer(member_schema, self)?;
            } else {
                self.skip_value()?;
            }
        }

        Ok(())
    }

    fn read_list(
        &mut self,
        _schema: &Schema,
        consumer: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        self.skip_whitespace();
        if self.remaining().first() != Some(&b'[') {
            return Err(SerdeError::TypeMismatch {
                message: "expected array".into(),
            });
        }
        self.advance_by(1);

        loop {
            self.skip_whitespace();
            if self.remaining().first() == Some(&b']') {
                self.advance_by(1);
                break;
            }
            consumer(self)?;
        }

        Ok(())
    }

    fn read_map(
        &mut self,
        _schema: &Schema,
        consumer: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        self.skip_whitespace();
        if self.remaining().first() != Some(&b'{') {
            return Err(SerdeError::TypeMismatch {
                message: "expected object".into(),
            });
        }
        self.advance_by(1);

        loop {
            self.skip_whitespace();
            if self.remaining().first() == Some(&b'}') {
                self.advance_by(1);
                break;
            }

            if self.remaining().first() != Some(&b'"') {
                return Err(SerdeError::InvalidInput {
                    message: "expected key".into(),
                });
            }

            let key = self.parse_key()?;

            self.skip_whitespace();
            if self.remaining().first() != Some(&b':') {
                return Err(SerdeError::InvalidInput {
                    message: "expected colon".into(),
                });
            }
            self.advance_by(1);
            self.skip_whitespace();

            consumer(key, self)?;
        }

        Ok(())
    }

    fn read_boolean(&mut self, _schema: &Schema) -> Result<bool, SerdeError> {
        self.skip_whitespace();
        let rem = self.remaining();
        if rem.starts_with(b"true") {
            self.advance_by(4);
            Ok(true)
        } else if rem.starts_with(b"false") {
            self.advance_by(5);
            Ok(false)
        } else {
            Err(SerdeError::TypeMismatch {
                message: "expected boolean".into(),
            })
        }
    }

    fn read_byte(&mut self, _schema: &Schema) -> Result<i8, SerdeError> {
        self.read_integer_value().and_then(|n| {
            i8::try_from(n).map_err(|_| SerdeError::InvalidInput {
                message: "value out of range for byte".into(),
            })
        })
    }

    fn read_short(&mut self, _schema: &Schema) -> Result<i16, SerdeError> {
        self.read_integer_value().and_then(|n| {
            i16::try_from(n).map_err(|_| SerdeError::InvalidInput {
                message: "value out of range for short".into(),
            })
        })
    }

    fn read_integer(&mut self, _schema: &Schema) -> Result<i32, SerdeError> {
        self.read_integer_value().and_then(|n| {
            i32::try_from(n).map_err(|_| SerdeError::InvalidInput {
                message: "value out of range for integer".into(),
            })
        })
    }

    fn read_long(&mut self, _schema: &Schema) -> Result<i64, SerdeError> {
        self.read_integer_value()
    }

    fn read_float(&mut self, _schema: &Schema) -> Result<f32, SerdeError> {
        self.read_float_value().map(|f| f as f32)
    }

    fn read_double(&mut self, _schema: &Schema) -> Result<f64, SerdeError> {
        self.read_float_value()
    }

    fn read_big_integer(&mut self, _schema: &Schema) -> Result<BigInteger, SerdeError> {
        use std::str::FromStr;
        let mut iter = json_token_iter(self.remaining());
        match iter.next() {
            Some(Ok(Token::ValueNumber { .. })) => {
                self.consume_number();
                BigInteger::from_str("0").map_err(|e| SerdeError::InvalidInput {
                    message: e.to_string(),
                })
            }
            _ => Err(SerdeError::TypeMismatch {
                message: "expected number".into(),
            }),
        }
    }

    fn read_big_decimal(&mut self, _schema: &Schema) -> Result<BigDecimal, SerdeError> {
        use std::str::FromStr;
        let mut iter = json_token_iter(self.remaining());
        match iter.next() {
            Some(Ok(Token::ValueNumber { .. })) => {
                self.consume_number();
                BigDecimal::from_str("0").map_err(|e| SerdeError::InvalidInput {
                    message: e.to_string(),
                })
            }
            _ => Err(SerdeError::TypeMismatch {
                message: "expected number".into(),
            }),
        }
    }

    fn read_string(&mut self, _schema: &Schema) -> Result<String, SerdeError> {
        self.skip_whitespace();
        let pos = self.position;
        let input = self.input;
        let rem = &input[pos..];
        if rem.first() != Some(&b'"') {
            return Err(SerdeError::TypeMismatch {
                message: "expected string".into(),
            });
        }
        // Scan for end of string, tracking whether escapes are present
        let mut i = 1;
        let mut has_escape = false;
        while i < rem.len() {
            if rem[i] == b'\\' {
                has_escape = true;
                i += 2;
            } else if rem[i] == b'"' {
                let raw = &input[pos + 1..pos + i];
                self.position = pos + i + 1;
                if !has_escape {
                    return std::str::from_utf8(raw).map(|s| s.to_owned()).map_err(|e| {
                        SerdeError::InvalidInput {
                            message: e.to_string(),
                        }
                    });
                }
                let s = std::str::from_utf8(raw).map_err(|e| SerdeError::InvalidInput {
                    message: e.to_string(),
                })?;
                return crate::deserialize::EscapedStr::new(s)
                    .to_unescaped()
                    .map(|s| s.into_owned())
                    .map_err(|e| SerdeError::InvalidInput {
                        message: e.to_string(),
                    });
            } else {
                i += 1;
            }
        }
        Err(SerdeError::InvalidInput {
            message: "unterminated string".into(),
        })
    }

    fn read_blob(&mut self, _schema: &Schema) -> Result<Blob, SerdeError> {
        let s = self.read_string(_schema)?;
        let decoded =
            aws_smithy_types::base64::decode(&s).map_err(|e| SerdeError::InvalidInput {
                message: format!("invalid base64: {}", e),
            })?;
        Ok(Blob::new(decoded))
    }

    fn read_string_list(&mut self, _schema: &Schema) -> Result<Vec<String>, SerdeError> {
        self.skip_whitespace();
        if self.remaining().first() != Some(&b'[') {
            return Err(SerdeError::TypeMismatch {
                message: "expected array".into(),
            });
        }
        self.advance_by(1);
        let mut out = Vec::new();
        loop {
            self.skip_whitespace();
            if self.remaining().first() == Some(&b']') {
                self.advance_by(1);
                break;
            }
            out.push(self.read_string(_schema)?);
        }
        Ok(out)
    }

    fn read_blob_list(&mut self, _schema: &Schema) -> Result<Vec<Blob>, SerdeError> {
        self.skip_whitespace();
        if self.remaining().first() != Some(&b'[') {
            return Err(SerdeError::TypeMismatch {
                message: "expected array".into(),
            });
        }
        self.advance_by(1);
        let mut out = Vec::new();
        loop {
            self.skip_whitespace();
            if self.remaining().first() == Some(&b']') {
                self.advance_by(1);
                break;
            }
            out.push(self.read_blob(_schema)?);
        }
        Ok(out)
    }

    fn read_integer_list(&mut self, _schema: &Schema) -> Result<Vec<i32>, SerdeError> {
        self.skip_whitespace();
        if self.remaining().first() != Some(&b'[') {
            return Err(SerdeError::TypeMismatch {
                message: "expected array".into(),
            });
        }
        self.advance_by(1);
        let mut out = Vec::new();
        loop {
            self.skip_whitespace();
            if self.remaining().first() == Some(&b']') {
                self.advance_by(1);
                break;
            }
            out.push(self.read_integer(_schema)?);
        }
        Ok(out)
    }

    fn read_long_list(&mut self, _schema: &Schema) -> Result<Vec<i64>, SerdeError> {
        self.skip_whitespace();
        if self.remaining().first() != Some(&b'[') {
            return Err(SerdeError::TypeMismatch {
                message: "expected array".into(),
            });
        }
        self.advance_by(1);
        let mut out = Vec::new();
        loop {
            self.skip_whitespace();
            if self.remaining().first() == Some(&b']') {
                self.advance_by(1);
                break;
            }
            out.push(self.read_long(_schema)?);
        }
        Ok(out)
    }

    fn read_string_string_map(
        &mut self,
        _schema: &Schema,
    ) -> Result<std::collections::HashMap<String, String>, SerdeError> {
        self.skip_whitespace();
        if self.remaining().first() != Some(&b'{') {
            return Err(SerdeError::TypeMismatch {
                message: "expected object".into(),
            });
        }
        self.advance_by(1);
        let mut out = std::collections::HashMap::new();
        loop {
            self.skip_whitespace();
            if self.remaining().first() == Some(&b'}') {
                self.advance_by(1);
                break;
            }
            if self.remaining().first() != Some(&b'"') {
                return Err(SerdeError::InvalidInput {
                    message: "expected key".into(),
                });
            }
            let key = self.parse_key()?;
            self.skip_whitespace();
            if self.remaining().first() != Some(&b':') {
                return Err(SerdeError::InvalidInput {
                    message: "expected colon".into(),
                });
            }
            self.advance_by(1);
            self.skip_whitespace();
            let val = self.read_string(_schema)?;
            out.insert(key, val);
        }
        Ok(out)
    }

    fn read_timestamp(&mut self, _schema: &Schema) -> Result<DateTime, SerdeError> {
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
            Some(Ok(Token::ValueNumber {
                value: Number::Float(f),
                ..
            })) => {
                self.consume_number();
                Ok(DateTime::from_secs_f64(f))
            }
            Some(Ok(Token::ValueString { .. })) => {
                // String timestamps — parse as date-time format
                let s = self.read_string(_schema)?;
                DateTime::from_str(&s, aws_smithy_types::date_time::Format::DateTime)
                    .map_err(|e| SerdeError::custom(format!("invalid timestamp string: {e}")))
            }
            _ => Err(SerdeError::TypeMismatch {
                message: "expected timestamp".into(),
            }),
        }
    }

    fn read_document(&mut self, _schema: &Schema) -> Result<Document, SerdeError> {
        let mut iter = json_token_iter(self.remaining());
        match iter.next() {
            Some(Ok(Token::ValueNull { .. })) => {
                self.advance_by(4);
                Ok(Document::Null)
            }
            _ => Err(SerdeError::UnsupportedOperation {
                message: "document deserialization not fully implemented".into(),
            }),
        }
    }

    fn is_null(&self) -> bool {
        let remaining = self.remaining();
        remaining.len() >= 4
            && &remaining[..4] == b"null"
            && !remaining.get(4).is_some_and(|b| b.is_ascii_alphanumeric())
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

    fn skip_value(&mut self) -> Result<(), SerdeError> {
        self.skip_whitespace();
        let mut depth: usize = 0;
        loop {
            self.skip_whitespace();
            match self.remaining().first().copied() {
                Some(b'{') | Some(b'[') => {
                    self.advance_by(1);
                    depth += 1;
                }
                Some(b'}') | Some(b']') => {
                    if depth == 0 {
                        return Err(SerdeError::InvalidInput {
                            message: "unexpected end token".into(),
                        });
                    }
                    self.advance_by(1);
                    depth -= 1;
                    if depth == 0 {
                        return Ok(());
                    }
                }
                Some(b'"') => {
                    // Skip quoted string (handles escapes)
                    let mut i = 1;
                    let rem = self.remaining();
                    while i < rem.len() {
                        if rem[i] == b'\\' {
                            i += 2; // skip escape sequence
                        } else if rem[i] == b'"' {
                            i += 1;
                            break;
                        } else {
                            i += 1;
                        }
                    }
                    self.advance_by(i);
                    // After a string inside an object, skip optional ':'
                    if depth > 0 {
                        self.skip_whitespace();
                        if self.remaining().first() == Some(&b':') {
                            self.advance_by(1);
                            continue; // read the value after the colon
                        }
                    }
                    if depth == 0 {
                        return Ok(());
                    }
                }
                Some(b't') => {
                    self.advance_by(4); // true
                    if depth == 0 {
                        return Ok(());
                    }
                }
                Some(b'f') => {
                    self.advance_by(5); // false
                    if depth == 0 {
                        return Ok(());
                    }
                }
                Some(b'n') => {
                    self.advance_by(4); // null
                    if depth == 0 {
                        return Ok(());
                    }
                }
                Some(c) if c == b'-' || c.is_ascii_digit() => {
                    self.consume_number();
                    if depth == 0 {
                        return Ok(());
                    }
                }
                Some(_) => {
                    return Err(SerdeError::InvalidInput {
                        message: "unexpected token in skip_value".into(),
                    })
                }
                None => {
                    return Err(SerdeError::InvalidInput {
                        message: "unexpected end of input".into(),
                    })
                }
            }
        }
    }

    fn read_integer_value(&mut self) -> Result<i64, SerdeError> {
        self.skip_whitespace();
        let rem = self.remaining();
        let mut len = 0;
        for &b in rem {
            if b.is_ascii_digit() || b == b'-' || b == b'+' {
                len += 1;
            } else {
                break;
            }
        }
        if len == 0 {
            return Err(SerdeError::TypeMismatch {
                message: "expected integer".into(),
            });
        }
        let s = std::str::from_utf8(&rem[..len]).map_err(|e| SerdeError::InvalidInput {
            message: e.to_string(),
        })?;
        let n = s.parse::<i64>().map_err(|e| SerdeError::InvalidInput {
            message: e.to_string(),
        })?;
        self.advance_by(len);
        Ok(n)
    }

    fn read_float_value(&mut self) -> Result<f64, SerdeError> {
        self.skip_whitespace();
        let rem = self.remaining();
        let mut len = 0;
        for &b in rem {
            if b.is_ascii_digit() || b == b'-' || b == b'+' || b == b'.' || b == b'e' || b == b'E' {
                len += 1;
            } else {
                break;
            }
        }
        if len == 0 {
            return Err(SerdeError::TypeMismatch {
                message: "expected number".into(),
            });
        }
        let s = std::str::from_utf8(&rem[..len]).map_err(|e| SerdeError::InvalidInput {
            message: e.to_string(),
        })?;
        let n = s.parse::<f64>().map_err(|e| SerdeError::InvalidInput {
            message: e.to_string(),
        })?;
        self.advance_by(len);
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_schema() -> &'static aws_smithy_schema::Schema {
        &aws_smithy_schema::prelude::STRING
    }

    #[test]
    fn test_read_boolean() {
        let mut deser = JsonDeserializer::new(b"true", Arc::new(JsonCodecSettings::default()));
        assert!(deser.read_boolean(dummy_schema()).unwrap());

        let mut deser = JsonDeserializer::new(b"false", Arc::new(JsonCodecSettings::default()));
        assert!(!(deser.read_boolean(dummy_schema()).unwrap()));
    }

    #[test]
    fn test_read_integer() {
        let mut deser = JsonDeserializer::new(b"42", Arc::new(JsonCodecSettings::default()));
        assert_eq!(deser.read_integer(dummy_schema()).unwrap(), 42);

        let mut deser = JsonDeserializer::new(b"-123", Arc::new(JsonCodecSettings::default()));
        assert_eq!(deser.read_integer(dummy_schema()).unwrap(), -123);
    }

    #[test]
    fn test_read_long() {
        let mut deser = JsonDeserializer::new(
            b"9223372036854775807",
            Arc::new(JsonCodecSettings::default()),
        );
        assert_eq!(deser.read_long(dummy_schema()).unwrap(), i64::MAX);
    }

    #[test]
    fn test_read_float() {
        let mut deser = JsonDeserializer::new(b"3.15", Arc::new(JsonCodecSettings::default()));
        assert!((deser.read_float(dummy_schema()).unwrap() - 3.15).abs() < 0.01);
    }

    #[test]
    fn test_read_double() {
        let mut deser = JsonDeserializer::new(b"2.72", Arc::new(JsonCodecSettings::default()));
        assert!((deser.read_double(dummy_schema()).unwrap() - 2.72).abs() < 0.001);
    }

    #[test]
    fn test_read_string() {
        let mut deser =
            JsonDeserializer::new(br#""hello world""#, Arc::new(JsonCodecSettings::default()));
        assert_eq!(deser.read_string(dummy_schema()).unwrap(), "hello world");

        let mut deser =
            JsonDeserializer::new(br#""hello\nworld""#, Arc::new(JsonCodecSettings::default()));
        assert_eq!(deser.read_string(dummy_schema()).unwrap(), "hello\nworld");
    }

    #[test]
    fn test_is_null() {
        let deser = JsonDeserializer::new(b"null", Arc::new(JsonCodecSettings::default()));
        assert!(deser.is_null());

        let deser = JsonDeserializer::new(b"42", Arc::new(JsonCodecSettings::default()));
        assert!(!deser.is_null());
    }

    #[test]
    fn test_read_byte_range() {
        let mut deser = JsonDeserializer::new(b"127", Arc::new(JsonCodecSettings::default()));
        assert_eq!(deser.read_byte(dummy_schema()).unwrap(), 127);

        let mut deser = JsonDeserializer::new(b"128", Arc::new(JsonCodecSettings::default()));
        assert!(deser.read_byte(dummy_schema()).is_err());
    }

    #[test]
    fn test_read_struct() {
        use aws_smithy_schema::Schema;

        #[derive(Debug, Default, PartialEq)]
        struct Person {
            first_name: String,
            last_name: String,
            age: i32,
        }

        static FIRST_NAME: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Person"),
            aws_smithy_schema::ShapeType::String,
            "firstName",
            0,
        );
        static LAST_NAME: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Person"),
            aws_smithy_schema::ShapeType::String,
            "lastName",
            1,
        );
        static AGE: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Person"),
            aws_smithy_schema::ShapeType::Integer,
            "age",
            2,
        );
        static PERSON_SCHEMA: Schema = Schema::new_struct(
            aws_smithy_schema::shape_id!("test", "Person"),
            aws_smithy_schema::ShapeType::Structure,
            &[&FIRST_NAME, &LAST_NAME, &AGE],
        );

        fn consume_person(
            person: &mut Person,
            schema: &Schema,
            deser: &mut dyn ShapeDeserializer,
        ) -> Result<(), SerdeError> {
            match schema.member_name() {
                Some("firstName") => person.first_name = deser.read_string(schema)?,
                Some("lastName") => person.last_name = deser.read_string(schema)?,
                Some("age") => person.age = deser.read_integer(schema)?,
                _ => {}
            }
            Ok(())
        }

        let json = br#"{"lastName":"Smithy","firstName":"Alice","age":30}"#;
        let mut deser = JsonDeserializer::new(json, Arc::new(JsonCodecSettings::default()));
        let mut person = Person::default();
        deser
            .read_struct(&PERSON_SCHEMA, &mut |member, d| {
                consume_person(&mut person, member, d)
            })
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
        let mut deser = JsonDeserializer::new(json, Arc::new(JsonCodecSettings::default()));
        let mut person = Person::default();
        deser
            .read_struct(&PERSON_SCHEMA, &mut |member, d| {
                consume_person(&mut person, member, d)
            })
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
        let mut deser = JsonDeserializer::new(json, Arc::new(JsonCodecSettings::default()));
        let capacity = deser.container_size().unwrap_or(0);
        let mut result = Vec::with_capacity(capacity);
        let allocated_capacity = result.capacity();
        deser
            .read_list(dummy_schema(), &mut |deser| {
                result.push(deser.read_integer(dummy_schema())?);
                Ok(())
            })
            .unwrap();
        assert_eq!(result, vec![1, 2, 3, 4, 5]);
        // Ensure no more memory was allocated for the container
        assert_eq!(result.capacity(), allocated_capacity);

        let json = b"[]";
        let mut deser = JsonDeserializer::new(json, Arc::new(JsonCodecSettings::default()));
        let capacity = deser.container_size().unwrap_or(0);
        let mut result = Vec::<i32>::with_capacity(capacity);
        let allocated_capacity = result.capacity();
        deser
            .read_list(dummy_schema(), &mut |deser| {
                result.push(deser.read_integer(dummy_schema())?);
                Ok(())
            })
            .unwrap();
        assert_eq!(result, Vec::<i32>::new());
        // Ensure no more memory was allocated for the container
        assert_eq!(result.capacity(), allocated_capacity);

        let json = br#"["hello", "world"]"#;
        let mut deser = JsonDeserializer::new(json, Arc::new(JsonCodecSettings::default()));
        let capacity = deser.container_size().unwrap_or(0);
        let mut result = Vec::with_capacity(capacity);
        let allocated_capacity = result.capacity();
        deser
            .read_list(dummy_schema(), &mut |deser| {
                result.push(deser.read_string(dummy_schema())?);
                Ok(())
            })
            .unwrap();
        assert_eq!(result, vec!["hello", "world"]);
        // Ensure no more memory was allocated for the container
        assert_eq!(result.capacity(), allocated_capacity);
    }

    #[test]
    fn test_container_size() {
        let deser =
            JsonDeserializer::new(b"[1, 2, 3, 4, 5]", Arc::new(JsonCodecSettings::default()));
        assert_eq!(deser.container_size(), Some(5));

        let deser = JsonDeserializer::new(b"[]", Arc::new(JsonCodecSettings::default()));
        assert_eq!(deser.container_size(), Some(0));

        let deser = JsonDeserializer::new(
            br#"{"a": 1, "b": 2, "c": 3}"#,
            Arc::new(JsonCodecSettings::default()),
        );
        assert_eq!(deser.container_size(), Some(3));

        let deser = JsonDeserializer::new(b"{}", Arc::new(JsonCodecSettings::default()));
        assert_eq!(deser.container_size(), Some(0));

        let deser = JsonDeserializer::new(
            b"[[1, 2], [3, 4], [5, 6]]",
            Arc::new(JsonCodecSettings::default()),
        );
        assert_eq!(deser.container_size(), Some(3));

        let deser = JsonDeserializer::new(b"42", Arc::new(JsonCodecSettings::default()));
        assert_eq!(deser.container_size(), None);
    }

    #[test]
    fn test_read_map() {
        use std::collections::HashMap;

        let json = br#"{"a": 1, "b": 2, "c": 3}"#;
        let mut deser = JsonDeserializer::new(json, Arc::new(JsonCodecSettings::default()));
        let calculated_capacity = deser.container_size().unwrap_or(0);
        let mut result = HashMap::with_capacity(calculated_capacity);
        let allocated_capacity = result.capacity();
        deser
            .read_map(dummy_schema(), &mut |key, deser| {
                result.insert(key, deser.read_integer(dummy_schema())?);
                Ok(())
            })
            .unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result.get("a"), Some(&1));
        assert_eq!(result.get("b"), Some(&2));
        assert_eq!(result.get("c"), Some(&3));
        // Ensure no more memory was allocated for the container
        assert_eq!(result.capacity(), allocated_capacity);

        let json = b"{}";
        let mut deser = JsonDeserializer::new(json, Arc::new(JsonCodecSettings::default()));
        let calculated_capacity = deser.container_size().unwrap_or(0);
        let mut result = HashMap::<String, i32>::with_capacity(calculated_capacity);
        let allocated_capacity = result.capacity();
        deser
            .read_map(dummy_schema(), &mut |key, deser| {
                result.insert(key, deser.read_integer(dummy_schema())?);
                Ok(())
            })
            .unwrap();
        assert_eq!(result, HashMap::<String, i32>::new());
        // Ensure no more memory was allocated for the container
        assert_eq!(result.capacity(), allocated_capacity);

        let json = br#"{"name": "Alice", "city": "Seattle"}"#;
        let mut deser = JsonDeserializer::new(json, Arc::new(JsonCodecSettings::default()));
        let calculated_capacity = deser.container_size().unwrap_or(0);
        let mut result = HashMap::with_capacity(calculated_capacity);
        let allocated_capacity = result.capacity();
        deser
            .read_map(dummy_schema(), &mut |key, deser| {
                result.insert(key, deser.read_string(dummy_schema())?);
                Ok(())
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
        use aws_smithy_schema::Schema;
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

        // Address members & schema
        static ADDR_STREET: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Address"),
            aws_smithy_schema::ShapeType::String,
            "street",
            0,
        );
        static ADDR_CITY: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Address"),
            aws_smithy_schema::ShapeType::String,
            "city",
            1,
        );
        static ADDR_ZIP: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Address"),
            aws_smithy_schema::ShapeType::Integer,
            "zip",
            2,
        );
        static ADDRESS_SCHEMA: Schema = Schema::new_struct(
            aws_smithy_schema::shape_id!("test", "Address"),
            aws_smithy_schema::ShapeType::Structure,
            &[&ADDR_STREET, &ADDR_CITY, &ADDR_ZIP],
        );

        // Company members & schema
        static COMP_NAME: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Company"),
            aws_smithy_schema::ShapeType::String,
            "name",
            0,
        );
        static COMP_EMPLOYEES: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Company"),
            aws_smithy_schema::ShapeType::List,
            "employees",
            1,
        );
        static COMP_METADATA: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Company"),
            aws_smithy_schema::ShapeType::Map,
            "metadata",
            2,
        );
        static COMP_ACTIVE: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Company"),
            aws_smithy_schema::ShapeType::Boolean,
            "active",
            3,
        );
        static COMPANY_SCHEMA: Schema = Schema::new_struct(
            aws_smithy_schema::shape_id!("test", "Company"),
            aws_smithy_schema::ShapeType::Structure,
            &[&COMP_NAME, &COMP_EMPLOYEES, &COMP_METADATA, &COMP_ACTIVE],
        );

        // User members & schema
        static USER_ID: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "User"),
            aws_smithy_schema::ShapeType::Long,
            "id",
            0,
        );
        static USER_NAME: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "User"),
            aws_smithy_schema::ShapeType::String,
            "name",
            1,
        );
        static USER_SCORES: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "User"),
            aws_smithy_schema::ShapeType::List,
            "scores",
            2,
        );
        static USER_ADDRESS: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "User"),
            aws_smithy_schema::ShapeType::Structure,
            "address",
            3,
        );
        static USER_COMPANIES: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "User"),
            aws_smithy_schema::ShapeType::List,
            "companies",
            4,
        );
        static USER_TAGS: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "User"),
            aws_smithy_schema::ShapeType::Map,
            "tags",
            5,
        );
        static USER_SCHEMA: Schema = Schema::new_struct(
            aws_smithy_schema::shape_id!("test", "User"),
            aws_smithy_schema::ShapeType::Structure,
            &[
                &USER_ID,
                &USER_NAME,
                &USER_SCORES,
                &USER_ADDRESS,
                &USER_COMPANIES,
                &USER_TAGS,
            ],
        );

        fn consume_address(
            addr: &mut Address,
            schema: &Schema,
            deser: &mut dyn ShapeDeserializer,
        ) -> Result<(), SerdeError> {
            match schema.member_name() {
                Some("street") => addr.street = deser.read_string(schema)?,
                Some("city") => addr.city = deser.read_string(schema)?,
                Some("zip") => addr.zip = deser.read_integer(schema)?,
                _ => {}
            }
            Ok(())
        }

        fn consume_company(
            comp: &mut Company,
            schema: &Schema,
            deser: &mut dyn ShapeDeserializer,
        ) -> Result<(), SerdeError> {
            match schema.member_name() {
                Some("name") => comp.name = deser.read_string(schema)?,
                Some("active") => comp.active = deser.read_boolean(schema)?,
                Some("employees") => {
                    let mut v = Vec::new();
                    deser.read_list(schema, &mut |d| {
                        v.push(d.read_string(dummy_schema())?);
                        Ok(())
                    })?;
                    comp.employees = v;
                }
                Some("metadata") => {
                    let mut m = HashMap::new();
                    deser.read_map(schema, &mut |k, d| {
                        m.insert(k, d.read_integer(dummy_schema())?);
                        Ok(())
                    })?;
                    comp.metadata = m;
                }
                _ => {}
            }
            Ok(())
        }

        fn consume_user(
            user: &mut User,
            schema: &Schema,
            deser: &mut dyn ShapeDeserializer,
        ) -> Result<(), SerdeError> {
            match schema.member_name() {
                Some("id") => user.id = deser.read_long(schema)?,
                Some("name") => user.name = deser.read_string(schema)?,
                Some("scores") => {
                    let mut v = Vec::new();
                    deser.read_list(schema, &mut |d| {
                        v.push(d.read_double(dummy_schema())?);
                        Ok(())
                    })?;
                    user.scores = v;
                }
                Some("address") => {
                    let mut addr = Address::default();
                    deser.read_struct(&ADDRESS_SCHEMA, &mut |member, d| {
                        consume_address(&mut addr, member, d)
                    })?;
                    user.address = addr;
                }
                Some("companies") => {
                    let mut v = Vec::new();
                    deser.read_list(schema, &mut |d| {
                        let mut comp = Company::default();
                        d.read_struct(&COMPANY_SCHEMA, &mut |member, d| {
                            consume_company(&mut comp, member, d)
                        })?;
                        v.push(comp);
                        Ok(())
                    })?;
                    user.companies = v;
                }
                Some("tags") => {
                    let mut m = HashMap::new();
                    deser.read_map(schema, &mut |k, d| {
                        m.insert(k, d.read_string(dummy_schema())?);
                        Ok(())
                    })?;
                    user.tags = m;
                }
                _ => {}
            }
            Ok(())
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

        let mut deser = JsonDeserializer::new(json, Arc::new(JsonCodecSettings::default()));
        let mut user = User::default();
        deser
            .read_struct(&USER_SCHEMA, &mut |member, d| {
                consume_user(&mut user, member, d)
            })
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
        assert!(user.companies[0].active);
        assert_eq!(user.companies[1].name, "StartupInc");
        assert_eq!(user.companies[1].employees, vec!["Charlie"]);
        assert_eq!(user.companies[1].metadata.get("founded"), Some(&2020));
        assert!(!user.companies[1].active);
        assert_eq!(user.tags.get("role"), Some(&"admin".to_string()));
        assert_eq!(user.tags.get("level"), Some(&"senior".to_string()));
    }

    #[test]
    fn test_json_name_deserialization() {
        use aws_smithy_schema::Schema;

        static FOO_MEMBER: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "MyStruct"),
            aws_smithy_schema::ShapeType::String,
            "foo",
            0,
        );
        // "bar" member has @jsonName("Baz")
        static BAR_MEMBER: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "MyStruct"),
            aws_smithy_schema::ShapeType::Integer,
            "bar",
            1,
        )
        .with_json_name("Baz");
        static STRUCT_SCHEMA: Schema = Schema::new_struct(
            aws_smithy_schema::shape_id!("test", "MyStruct"),
            aws_smithy_schema::ShapeType::Structure,
            &[&FOO_MEMBER, &BAR_MEMBER],
        );

        let json = br#"{"foo":"hello","Baz":42}"#;

        // With use_json_name=true, "Baz" resolves to the "bar" member
        let mut deser = JsonDeserializer::new(json, Arc::new(JsonCodecSettings::default()));
        let (mut foo, mut bar) = (None::<String>, None::<i32>);
        deser
            .read_struct(&STRUCT_SCHEMA, &mut |member, d| {
                match member.member_name() {
                    Some("foo") => foo = Some(d.read_string(member)?),
                    Some("bar") => bar = Some(d.read_integer(member)?),
                    _ => {}
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(foo.as_deref(), Some("hello"));
        assert_eq!(bar, Some(42));

        // With use_json_name=false, "Baz" is unknown and gets skipped
        let mut deser = JsonDeserializer::new(
            json,
            Arc::new(JsonCodecSettings::builder().use_json_name(false).build()),
        );
        let (mut foo, mut bar) = (None::<String>, None::<i32>);
        deser
            .read_struct(&STRUCT_SCHEMA, &mut |member, d| {
                match member.member_name() {
                    Some("foo") => foo = Some(d.read_string(member)?),
                    Some("bar") => bar = Some(d.read_integer(member)?),
                    _ => {}
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(foo.as_deref(), Some("hello"));
        assert_eq!(bar, None); // "Baz" not recognized without jsonName
    }

    fn timestamp_schema() -> &'static aws_smithy_schema::Schema {
        &aws_smithy_schema::prelude::TIMESTAMP
    }

    #[test]
    fn test_read_timestamp_positive_integer() {
        let mut deser =
            JsonDeserializer::new(b"1700000000", Arc::new(JsonCodecSettings::default()));
        let ts = deser.read_timestamp(timestamp_schema()).unwrap();
        assert_eq!(ts, DateTime::from_secs(1700000000));
    }

    #[test]
    fn test_read_timestamp_negative_integer() {
        let mut deser = JsonDeserializer::new(b"-1000", Arc::new(JsonCodecSettings::default()));
        let ts = deser.read_timestamp(timestamp_schema()).unwrap();
        assert_eq!(ts, DateTime::from_secs(-1000));
    }

    #[test]
    fn test_read_timestamp_float() {
        // This is the format DynamoDB uses: epoch seconds with fractional part
        let mut deser =
            JsonDeserializer::new(b"1.615218678973E9", Arc::new(JsonCodecSettings::default()));
        let ts = deser.read_timestamp(timestamp_schema()).unwrap();
        assert_eq!(ts, DateTime::from_secs_f64(1.615218678973E9));
    }

    #[test]
    fn test_read_timestamp_float_simple() {
        let mut deser =
            JsonDeserializer::new(b"1700000000.5", Arc::new(JsonCodecSettings::default()));
        let ts = deser.read_timestamp(timestamp_schema()).unwrap();
        assert_eq!(ts, DateTime::from_secs_f64(1700000000.5));
    }

    #[test]
    fn test_read_timestamp_string_datetime() {
        let mut deser = JsonDeserializer::new(
            br#""2023-11-14T22:13:20Z""#,
            Arc::new(JsonCodecSettings::default()),
        );
        let ts = deser.read_timestamp(timestamp_schema()).unwrap();
        assert_eq!(ts, DateTime::from_secs(1700000000));
    }

    #[test]
    fn test_read_timestamp_invalid() {
        let mut deser = JsonDeserializer::new(b"true", Arc::new(JsonCodecSettings::default()));
        assert!(deser.read_timestamp(timestamp_schema()).is_err());
    }

    #[test]
    fn test_skip_value_empty_array() {
        // Regression: skip_value failed on [] because json_token_iter can't parse ']' as a value start
        use aws_smithy_schema::ShapeType;
        static KNOWN_MEMBER: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "S"),
            ShapeType::String,
            "known",
            0,
        );
        static MEMBERS: &[&Schema] = &[&KNOWN_MEMBER];
        static TEST_SCHEMA: Schema = Schema::new_struct(
            aws_smithy_schema::shape_id!("test", "S"),
            ShapeType::Structure,
            MEMBERS,
        );

        let json = br#"{"known":"yes","Items":[],"extra":true}"#;
        let mut deser = JsonDeserializer::new(json, Arc::new(JsonCodecSettings::default()));
        let mut known_val = String::new();
        deser
            .read_struct(&TEST_SCHEMA, &mut |member, deser| {
                if member.member_name() == Some("known") {
                    known_val = deser.read_string(dummy_schema())?;
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(known_val, "yes");
    }

    #[test]
    fn test_skip_value_nested_objects() {
        use aws_smithy_schema::ShapeType;
        static D_MEMBER: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "S"),
            ShapeType::String,
            "d",
            0,
        );
        static MEMBERS: &[&Schema] = &[&D_MEMBER];
        static TEST_SCHEMA: Schema = Schema::new_struct(
            aws_smithy_schema::shape_id!("test", "S"),
            ShapeType::Structure,
            MEMBERS,
        );

        let json = br#"{"a":{"b":[1,2,{"c":3}]},"d":"ok"}"#;
        let mut deser = JsonDeserializer::new(json, Arc::new(JsonCodecSettings::default()));
        let mut d_val = String::new();
        deser
            .read_struct(&TEST_SCHEMA, &mut |member, deser| {
                if member.member_name() == Some("d") {
                    d_val = deser.read_string(dummy_schema())?;
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(d_val, "ok");
    }
}
