/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! JSON serializer implementation.

use aws_smithy_schema::serde::ShapeSerializer;
use aws_smithy_schema::{Schema, ShapeId};
use aws_smithy_types::date_time::Format as TimestampFormat;
use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Document};
use std::fmt;

use crate::codec::JsonCodecSettings;

/// Error type for JSON serialization.
#[derive(Debug)]
pub enum JsonSerializerError {
    /// An error occurred during JSON writing.
    WriteError(String),
}

impl fmt::Display for JsonSerializerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WriteError(msg) => write!(f, "JSON write error: {}", msg),
        }
    }
}

impl std::error::Error for JsonSerializerError {}

/// JSON serializer that implements the ShapeSerializer trait.
pub struct JsonSerializer {
    output: String,
    settings: JsonCodecSettings,
}

impl JsonSerializer {
    /// Creates a new JSON serializer with the given settings.
    pub fn new(settings: JsonCodecSettings) -> Self {
        Self {
            output: String::new(),
            settings,
        }
    }

    /// Gets the timestamp format to use, respecting @timestampFormat trait.
    fn get_timestamp_format(&self, schema: &dyn Schema) -> TimestampFormat {
        let timestamp_format_trait_id = ShapeId::new("smithy.api#timestampFormat");
        if let Some(trait_obj) = schema.traits().get(&timestamp_format_trait_id) {
            if let Some(format_str) = trait_obj.as_any().downcast_ref::<String>() {
                return match format_str.as_str() {
                    "epoch-seconds" => TimestampFormat::EpochSeconds,
                    "http-date" => TimestampFormat::HttpDate,
                    "date-time" => TimestampFormat::DateTime,
                    _ => self.settings.default_timestamp_format,
                };
            }
        }
        self.settings.default_timestamp_format
    }

    fn write_json_value(&mut self, doc: &Document) {
        use crate::serialize::JsonValueWriter;
        let writer = JsonValueWriter::new(&mut self.output);
        writer.document(doc);
    }
}

impl ShapeSerializer for JsonSerializer {
    type Output = Vec<u8>;
    type Error = JsonSerializerError;

    fn finish(self) -> Result<Self::Output, Self::Error> {
        Ok(self.output.into_bytes())
    }

    fn write_struct<F>(&mut self, _schema: &dyn Schema, write_members: F) -> Result<(), Self::Error>
    where
        F: FnOnce(&mut Self) -> Result<(), Self::Error>,
    {
        self.output.push('{');
        write_members(self)?;
        self.output.push('}');
        Ok(())
    }

    fn write_list<F>(&mut self, _schema: &dyn Schema, write_elements: F) -> Result<(), Self::Error>
    where
        F: FnOnce(&mut Self) -> Result<(), Self::Error>,
    {
        self.output.push('[');
        write_elements(self)?;
        self.output.push(']');
        Ok(())
    }

    fn write_map<F>(&mut self, _schema: &dyn Schema, write_entries: F) -> Result<(), Self::Error>
    where
        F: FnOnce(&mut Self) -> Result<(), Self::Error>,
    {
        self.output.push('{');
        write_entries(self)?;
        self.output.push('}');
        Ok(())
    }

    fn write_boolean(&mut self, _schema: &dyn Schema, value: bool) -> Result<(), Self::Error> {
        self.output.push_str(if value { "true" } else { "false" });
        Ok(())
    }

    fn write_byte(&mut self, _schema: &dyn Schema, value: i8) -> Result<(), Self::Error> {
        use std::fmt::Write;
        write!(&mut self.output, "{}", value)
            .map_err(|e| JsonSerializerError::WriteError(e.to_string()))
    }

    fn write_short(&mut self, _schema: &dyn Schema, value: i16) -> Result<(), Self::Error> {
        use std::fmt::Write;
        write!(&mut self.output, "{}", value)
            .map_err(|e| JsonSerializerError::WriteError(e.to_string()))
    }

    fn write_integer(&mut self, _schema: &dyn Schema, value: i32) -> Result<(), Self::Error> {
        use std::fmt::Write;
        write!(&mut self.output, "{}", value)
            .map_err(|e| JsonSerializerError::WriteError(e.to_string()))
    }

    fn write_long(&mut self, _schema: &dyn Schema, value: i64) -> Result<(), Self::Error> {
        use std::fmt::Write;
        write!(&mut self.output, "{}", value)
            .map_err(|e| JsonSerializerError::WriteError(e.to_string()))
    }

    fn write_float(&mut self, _schema: &dyn Schema, value: f32) -> Result<(), Self::Error> {
        use std::fmt::Write;
        write!(&mut self.output, "{}", value)
            .map_err(|e| JsonSerializerError::WriteError(e.to_string()))
    }

    fn write_double(&mut self, _schema: &dyn Schema, value: f64) -> Result<(), Self::Error> {
        use std::fmt::Write;
        write!(&mut self.output, "{}", value)
            .map_err(|e| JsonSerializerError::WriteError(e.to_string()))
    }

    fn write_big_integer(
        &mut self,
        _schema: &dyn Schema,
        value: &BigInteger,
    ) -> Result<(), Self::Error> {
        self.output.push_str(value.as_ref());
        Ok(())
    }

    fn write_big_decimal(
        &mut self,
        _schema: &dyn Schema,
        value: &BigDecimal,
    ) -> Result<(), Self::Error> {
        self.output.push_str(value.as_ref());
        Ok(())
    }

    fn write_string(&mut self, _schema: &dyn Schema, value: &str) -> Result<(), Self::Error> {
        use crate::escape::escape_string;
        self.output.push('"');
        self.output.push_str(&escape_string(value));
        self.output.push('"');
        Ok(())
    }

    fn write_blob(&mut self, _schema: &dyn Schema, value: &Blob) -> Result<(), Self::Error> {
        use aws_smithy_types::base64;
        let encoded = base64::encode(value.as_ref());
        self.output.push('"');
        self.output.push_str(&encoded);
        self.output.push('"');
        Ok(())
    }

    fn write_timestamp(
        &mut self,
        schema: &dyn Schema,
        value: &DateTime,
    ) -> Result<(), Self::Error> {
        let format = self.get_timestamp_format(schema);
        let formatted = value.fmt(format).map_err(|e| {
            JsonSerializerError::WriteError(format!("Failed to format timestamp: {}", e))
        })?;

        match format {
            TimestampFormat::EpochSeconds => {
                // Epoch seconds as number
                self.output.push_str(&formatted);
            }
            _ => {
                // Other formats as strings
                self.output.push('"');
                self.output.push_str(&formatted);
                self.output.push('"');
            }
        }
        Ok(())
    }

    fn write_document(
        &mut self,
        _schema: &dyn Schema,
        value: &Document,
    ) -> Result<(), Self::Error> {
        self.write_json_value(value);
        Ok(())
    }

    fn write_null(&mut self, _schema: &dyn Schema) -> Result<(), Self::Error> {
        self.output.push_str("null");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_schema::prelude::*;

    #[test]
    fn test_write_boolean() {
        let mut ser = JsonSerializer::new(JsonCodecSettings::default());
        ser.write_boolean(&BOOLEAN, true).unwrap();
        let output = ser.finish().unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "true");
    }

    #[test]
    fn test_write_string() {
        let mut ser = JsonSerializer::new(JsonCodecSettings::default());
        ser.write_string(&STRING, "hello").unwrap();
        let output = ser.finish().unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "\"hello\"");
    }

    #[test]
    fn test_write_integer() {
        let mut ser = JsonSerializer::new(JsonCodecSettings::default());
        ser.write_integer(&INTEGER, 42).unwrap();
        let output = ser.finish().unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "42");
    }

    #[test]
    fn test_write_null() {
        let mut ser = JsonSerializer::new(JsonCodecSettings::default());
        ser.write_null(&STRING).unwrap();
        let output = ser.finish().unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "null");
    }

    #[test]
    fn test_write_list() {
        let mut ser = JsonSerializer::new(JsonCodecSettings::default());
        // Create a simple list schema for testing
        let list_schema = aws_smithy_schema::prelude::PreludeSchema::new(
            aws_smithy_schema::ShapeId::new("test#List"),
            aws_smithy_schema::ShapeType::List,
        );
        ser.write_list(&list_schema, |s| {
            s.write_integer(&INTEGER, 1)?;
            s.output.push(',');
            s.write_integer(&INTEGER, 2)?;
            s.output.push(',');
            s.write_integer(&INTEGER, 3)?;
            Ok(())
        })
        .unwrap();
        let output = ser.finish().unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "[1,2,3]");
    }

    #[test]
    fn test_write_full_object() {
        let mut ser = JsonSerializer::new(JsonCodecSettings::default());
        let struct_schema = aws_smithy_schema::prelude::PreludeSchema::new(
            aws_smithy_schema::ShapeId::new("test#Struct"),
            aws_smithy_schema::ShapeType::Structure,
        );
        let list_schema = aws_smithy_schema::prelude::PreludeSchema::new(
            aws_smithy_schema::ShapeId::new("test#List"),
            aws_smithy_schema::ShapeType::List,
        );
        ser.write_struct(&struct_schema, |s| {
            s.output.push_str("\"active\":");
            s.write_boolean(&BOOLEAN, true)?;
            s.output.push(',');
            s.output.push_str("\"name\":");
            s.write_string(&STRING, "test")?;
            s.output.push(',');
            s.output.push_str("\"count\":");
            s.write_integer(&INTEGER, 42)?;
            s.output.push(',');
            s.output.push_str("\"price\":");
            s.write_float(&FLOAT, 3.14)?;
            s.output.push(',');
            s.output.push_str("\"items\":");
            s.write_list(&list_schema, |ls| {
                ls.write_integer(&INTEGER, 1)?;
                ls.output.push(',');
                ls.write_integer(&INTEGER, 2)?;
                Ok(())
            })?;
            Ok(())
        })
        .unwrap();
        let output = ser.finish().unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "{\"active\":true,\"name\":\"test\",\"count\":42,\"price\":3.14,\"items\":[1,2]}"
        );
    }
}
