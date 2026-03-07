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
    /// Tracks whether we need a comma before the next value in a struct/list/map.
    needs_comma: Vec<bool>,
}

impl JsonSerializer {
    /// Creates a new JSON serializer with the given settings.
    pub fn new(settings: JsonCodecSettings) -> Self {
        Self {
            output: String::new(),
            settings,
            needs_comma: Vec::new(),
        }
    }

    /// Writes a member name prefix (key + colon) if the schema is a member schema.
    fn write_member_prefix(&mut self, schema: &dyn Schema) {
        // Add comma separator if needed
        if let Some(needs) = self.needs_comma.last_mut() {
            if *needs {
                self.output.push(',');
            }
            *needs = true;
        }

        // Write member name if this is a member schema
        if let Some(name) = schema.member_name() {
            self.output.push('"');
            self.output.push_str(name);
            self.output.push_str("\":");
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

    fn write_struct<F>(&mut self, schema: &dyn Schema, write_members: F) -> Result<(), Self::Error>
    where
        F: FnOnce(&mut Self) -> Result<(), Self::Error>,
    {
        self.write_member_prefix(schema);
        self.output.push('{');
        self.needs_comma.push(false);
        write_members(self)?;
        self.needs_comma.pop();
        self.output.push('}');
        Ok(())
    }

    fn write_list<F>(&mut self, schema: &dyn Schema, write_elements: F) -> Result<(), Self::Error>
    where
        F: FnOnce(&mut Self) -> Result<(), Self::Error>,
    {
        self.write_member_prefix(schema);
        self.output.push('[');
        self.needs_comma.push(false);
        write_elements(self)?;
        self.needs_comma.pop();
        self.output.push(']');
        Ok(())
    }

    fn write_map<F>(&mut self, schema: &dyn Schema, write_entries: F) -> Result<(), Self::Error>
    where
        F: FnOnce(&mut Self) -> Result<(), Self::Error>,
    {
        self.write_member_prefix(schema);
        self.output.push('{');
        self.needs_comma.push(false);
        write_entries(self)?;
        self.needs_comma.pop();
        self.output.push('}');
        Ok(())
    }

    fn write_boolean(&mut self, schema: &dyn Schema, value: bool) -> Result<(), Self::Error> {
        self.write_member_prefix(schema);
        self.output.push_str(if value { "true" } else { "false" });
        Ok(())
    }

    fn write_byte(&mut self, schema: &dyn Schema, value: i8) -> Result<(), Self::Error> {
        self.write_member_prefix(schema);
        use std::fmt::Write;
        write!(&mut self.output, "{}", value)
            .map_err(|e| JsonSerializerError::WriteError(e.to_string()))
    }

    fn write_short(&mut self, schema: &dyn Schema, value: i16) -> Result<(), Self::Error> {
        self.write_member_prefix(schema);
        use std::fmt::Write;
        write!(&mut self.output, "{}", value)
            .map_err(|e| JsonSerializerError::WriteError(e.to_string()))
    }

    fn write_integer(&mut self, schema: &dyn Schema, value: i32) -> Result<(), Self::Error> {
        self.write_member_prefix(schema);
        use std::fmt::Write;
        write!(&mut self.output, "{}", value)
            .map_err(|e| JsonSerializerError::WriteError(e.to_string()))
    }

    fn write_long(&mut self, schema: &dyn Schema, value: i64) -> Result<(), Self::Error> {
        self.write_member_prefix(schema);
        use std::fmt::Write;
        write!(&mut self.output, "{}", value)
            .map_err(|e| JsonSerializerError::WriteError(e.to_string()))
    }

    fn write_float(&mut self, schema: &dyn Schema, value: f32) -> Result<(), Self::Error> {
        self.write_member_prefix(schema);
        use std::fmt::Write;
        write!(&mut self.output, "{}", value)
            .map_err(|e| JsonSerializerError::WriteError(e.to_string()))
    }

    fn write_double(&mut self, schema: &dyn Schema, value: f64) -> Result<(), Self::Error> {
        self.write_member_prefix(schema);
        use std::fmt::Write;
        write!(&mut self.output, "{}", value)
            .map_err(|e| JsonSerializerError::WriteError(e.to_string()))
    }

    fn write_big_integer(
        &mut self,
        schema: &dyn Schema,
        value: &BigInteger,
    ) -> Result<(), Self::Error> {
        self.write_member_prefix(schema);
        self.output.push_str(value.as_ref());
        Ok(())
    }

    fn write_big_decimal(
        &mut self,
        schema: &dyn Schema,
        value: &BigDecimal,
    ) -> Result<(), Self::Error> {
        self.write_member_prefix(schema);
        self.output.push_str(value.as_ref());
        Ok(())
    }

    fn write_string(&mut self, schema: &dyn Schema, value: &str) -> Result<(), Self::Error> {
        self.write_member_prefix(schema);
        use crate::escape::escape_string;
        self.output.push('"');
        self.output.push_str(&escape_string(value));
        self.output.push('"');
        Ok(())
    }

    fn write_blob(&mut self, schema: &dyn Schema, value: &Blob) -> Result<(), Self::Error> {
        self.write_member_prefix(schema);
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
        self.write_member_prefix(schema);
        let format = self.get_timestamp_format(schema);
        let formatted = value.fmt(format).map_err(|e| {
            JsonSerializerError::WriteError(format!("Failed to format timestamp: {}", e))
        })?;

        match format {
            TimestampFormat::EpochSeconds => {
                self.output.push_str(&formatted);
            }
            _ => {
                self.output.push('"');
                self.output.push_str(&formatted);
                self.output.push('"');
            }
        }
        Ok(())
    }

    fn write_document(&mut self, schema: &dyn Schema, value: &Document) -> Result<(), Self::Error> {
        self.write_member_prefix(schema);
        self.write_json_value(value);
        Ok(())
    }

    fn write_null(&mut self, schema: &dyn Schema) -> Result<(), Self::Error> {
        self.write_member_prefix(schema);
        self.output.push_str("null");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_schema::prelude::*;
    use aws_smithy_schema::MemberSchema;

    fn member(
        name: &'static str,
        shape_type: aws_smithy_schema::ShapeType,
        idx: usize,
    ) -> MemberSchema {
        MemberSchema::new(
            aws_smithy_schema::ShapeId::from_static("test#S$m", "test", "S"),
            shape_type,
            name,
            idx,
        )
    }

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
            s.write_integer(&INTEGER, 2)?;
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
        let active = member("active", aws_smithy_schema::ShapeType::Boolean, 0);
        let name = member("name", aws_smithy_schema::ShapeType::String, 1);
        let count = member("count", aws_smithy_schema::ShapeType::Integer, 2);
        let price = member("price", aws_smithy_schema::ShapeType::Float, 3);
        let items = member("items", aws_smithy_schema::ShapeType::List, 4);
        ser.write_struct(&struct_schema, |s| {
            s.write_boolean(&active, true)?;
            s.write_string(&name, "test")?;
            s.write_integer(&count, 42)?;
            s.write_float(&price, 3.14)?;
            s.write_list(&items, |ls| {
                ls.write_integer(&INTEGER, 1)?;
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

    #[test]
    fn test_nested_complex_serialization() {
        let mut ser = JsonSerializer::new(JsonCodecSettings::default());
        let struct_schema = aws_smithy_schema::prelude::PreludeSchema::new(
            aws_smithy_schema::ShapeId::new("test#User"),
            aws_smithy_schema::ShapeType::Structure,
        );
        // User members
        let id_m = member("id", aws_smithy_schema::ShapeType::Long, 0);
        let name_m = member("name", aws_smithy_schema::ShapeType::String, 1);
        let scores_m = member("scores", aws_smithy_schema::ShapeType::List, 2);
        let address_m = member("address", aws_smithy_schema::ShapeType::Structure, 3);
        let companies_m = member("companies", aws_smithy_schema::ShapeType::List, 4);
        let tags_m = member("tags", aws_smithy_schema::ShapeType::Map, 5);
        // Address members
        let street_m = member("street", aws_smithy_schema::ShapeType::String, 0);
        let city_m = member("city", aws_smithy_schema::ShapeType::String, 1);
        let zip_m = member("zip", aws_smithy_schema::ShapeType::Integer, 2);
        // Company members
        let cname_m = member("name", aws_smithy_schema::ShapeType::String, 0);
        let employees_m = member("employees", aws_smithy_schema::ShapeType::List, 1);
        let metadata_m = member("metadata", aws_smithy_schema::ShapeType::Map, 2);
        let active_m = member("active", aws_smithy_schema::ShapeType::Boolean, 3);
        // Map entry members
        let founded_m = member("founded", aws_smithy_schema::ShapeType::Integer, 0);
        let size_m = member("size", aws_smithy_schema::ShapeType::Integer, 1);
        let role_m = member("role", aws_smithy_schema::ShapeType::String, 0);
        let level_m = member("level", aws_smithy_schema::ShapeType::String, 1);

        ser.write_struct(&struct_schema, |s| {
            s.write_long(&id_m, 12345)?;
            s.write_string(&name_m, "John Doe")?;
            s.write_list(&scores_m, |ls| {
                ls.write_double(&DOUBLE, 95.5)?;
                ls.write_double(&DOUBLE, 87.3)?;
                ls.write_double(&DOUBLE, 92.1)?;
                Ok(())
            })?;
            s.write_struct(&address_m, |addr| {
                addr.write_string(&street_m, "123 Main St")?;
                addr.write_string(&city_m, "Seattle")?;
                addr.write_integer(&zip_m, 98101)?;
                Ok(())
            })?;
            s.write_list(&companies_m, |ls| {
                ls.write_struct(&struct_schema, |comp| {
                    comp.write_string(&cname_m, "TechCorp")?;
                    comp.write_list(&employees_m, |emp| {
                        emp.write_string(&STRING, "Alice")?;
                        emp.write_string(&STRING, "Bob")?;
                        Ok(())
                    })?;
                    comp.write_map(&metadata_m, |meta| {
                        meta.write_integer(&founded_m, 2010)?;
                        meta.write_integer(&size_m, 500)?;
                        Ok(())
                    })?;
                    comp.write_boolean(&active_m, true)?;
                    Ok(())
                })?;
                ls.write_struct(&struct_schema, |comp| {
                    comp.write_string(&cname_m, "StartupInc")?;
                    comp.write_list(&employees_m, |emp| {
                        emp.write_string(&STRING, "Charlie")?;
                        Ok(())
                    })?;
                    comp.write_map(&metadata_m, |meta| {
                        meta.write_integer(&founded_m, 2020)?;
                        Ok(())
                    })?;
                    comp.write_boolean(&active_m, false)?;
                    Ok(())
                })?;
                Ok(())
            })?;
            s.write_map(&tags_m, |tags| {
                tags.write_string(&role_m, "admin")?;
                tags.write_string(&level_m, "senior")?;
                Ok(())
            })?;
            Ok(())
        })
        .unwrap();

        let output = String::from_utf8(ser.finish().unwrap()).unwrap();
        let expected = r#"{"id":12345,"name":"John Doe","scores":[95.5,87.3,92.1],"address":{"street":"123 Main St","city":"Seattle","zip":98101},"companies":[{"name":"TechCorp","employees":["Alice","Bob"],"metadata":{"founded":2010,"size":500},"active":true},{"name":"StartupInc","employees":["Charlie"],"metadata":{"founded":2020},"active":false}],"tags":{"role":"admin","level":"senior"}}"#;
        assert_eq!(output, expected);
    }
}
