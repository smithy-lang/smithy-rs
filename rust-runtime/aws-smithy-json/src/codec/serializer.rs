/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! JSON serializer implementation.

use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::Schema;
use aws_smithy_types::date_time::Format as TimestampFormat;
use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Document};

use crate::codec::JsonCodecSettings;

use std::sync::Arc;

/// JSON serializer that implements the ShapeSerializer trait.
pub struct JsonSerializer {
    output: String,
    settings: Arc<JsonCodecSettings>,
    // Tracks whether a comma is needed before the next value in the current container.
    needs_comma: bool,
}

impl JsonSerializer {
    /// Creates a new JSON serializer with the given settings.
    pub(crate) fn new(settings: Arc<JsonCodecSettings>) -> Self {
        Self {
            output: String::new(),
            settings,
            needs_comma: false,
        }
    }

    /// Finalizes the serialization and returns the output as bytes.
    pub fn finish(self) -> Vec<u8> {
        self.output.into_bytes()
    }

    /// Inserts a comma separator if needed, then writes the member name if the
    /// schema is a member schema. When `use_json_name` is enabled, checks for
    /// a `@jsonName` trait and uses that value instead of the member name.
    fn prefix(&mut self, schema: &Schema) {
        if self.needs_comma {
            self.output.push(',');
        }
        if let Some(name) = self.field_name(schema) {
            self.output.push('"');
            self.output.push_str(name);
            self.output.push_str("\":");
        }
        self.needs_comma = true;
    }

    /// Resolves the JSON field name for a member schema.
    fn field_name<'a>(&self, schema: &'a Schema) -> Option<&'a str> {
        self.settings.member_to_field(schema)
    }

    /// Gets the timestamp format to use, respecting @timestampFormat trait.
    fn get_timestamp_format(&self, schema: &Schema) -> TimestampFormat {
        if let Some(trait_obj) = schema
            .traits()
            .get(&aws_smithy_schema::serde_traits::TIMESTAMP_FORMAT)
        {
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
    fn write_struct(
        &mut self,
        schema: &Schema,
        value: &dyn SerializableStruct,
    ) -> Result<(), SerdeError> {
        self.prefix(schema);
        self.output.push('{');
        let saved = self.needs_comma;
        self.needs_comma = false;
        value.serialize_members(self)?;
        self.output.push('}');
        self.needs_comma = saved;
        Ok(())
    }

    fn write_list(
        &mut self,
        schema: &Schema,
        write_elements: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        self.prefix(schema);
        self.output.push('[');
        let saved = self.needs_comma;
        self.needs_comma = false;
        write_elements(self)?;
        self.output.push(']');
        self.needs_comma = saved;
        Ok(())
    }

    fn write_map(
        &mut self,
        schema: &Schema,
        write_entries: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        self.prefix(schema);
        self.output.push('{');
        let saved = self.needs_comma;
        self.needs_comma = false;
        write_entries(self)?;
        self.output.push('}');
        self.needs_comma = saved;
        Ok(())
    }

    fn write_boolean(&mut self, schema: &Schema, value: bool) -> Result<(), SerdeError> {
        self.prefix(schema);
        self.output.push_str(if value { "true" } else { "false" });
        Ok(())
    }

    fn write_byte(&mut self, schema: &Schema, value: i8) -> Result<(), SerdeError> {
        use std::fmt::Write;
        self.prefix(schema);
        write!(&mut self.output, "{}", value).map_err(|e| SerdeError::WriteFailed {
            message: e.to_string(),
        })
    }

    fn write_short(&mut self, schema: &Schema, value: i16) -> Result<(), SerdeError> {
        use std::fmt::Write;
        self.prefix(schema);
        write!(&mut self.output, "{}", value).map_err(|e| SerdeError::WriteFailed {
            message: e.to_string(),
        })
    }

    fn write_integer(&mut self, schema: &Schema, value: i32) -> Result<(), SerdeError> {
        use std::fmt::Write;
        self.prefix(schema);
        write!(&mut self.output, "{}", value).map_err(|e| SerdeError::WriteFailed {
            message: e.to_string(),
        })
    }

    fn write_long(&mut self, schema: &Schema, value: i64) -> Result<(), SerdeError> {
        use std::fmt::Write;
        self.prefix(schema);
        write!(&mut self.output, "{}", value).map_err(|e| SerdeError::WriteFailed {
            message: e.to_string(),
        })
    }

    fn write_float(&mut self, schema: &Schema, value: f32) -> Result<(), SerdeError> {
        use std::fmt::Write;
        self.prefix(schema);
        write!(&mut self.output, "{}", value).map_err(|e| SerdeError::WriteFailed {
            message: e.to_string(),
        })
    }

    fn write_double(&mut self, schema: &Schema, value: f64) -> Result<(), SerdeError> {
        use std::fmt::Write;
        self.prefix(schema);
        write!(&mut self.output, "{}", value).map_err(|e| SerdeError::WriteFailed {
            message: e.to_string(),
        })
    }

    fn write_big_integer(&mut self, schema: &Schema, value: &BigInteger) -> Result<(), SerdeError> {
        self.prefix(schema);
        self.output.push_str(value.as_ref());
        Ok(())
    }

    fn write_big_decimal(&mut self, schema: &Schema, value: &BigDecimal) -> Result<(), SerdeError> {
        self.prefix(schema);
        self.output.push_str(value.as_ref());
        Ok(())
    }

    fn write_string(&mut self, schema: &Schema, value: &str) -> Result<(), SerdeError> {
        use crate::escape::escape_string;
        self.prefix(schema);
        self.output.push('"');
        self.output.push_str(&escape_string(value));
        self.output.push('"');
        Ok(())
    }

    fn write_blob(&mut self, schema: &Schema, value: &Blob) -> Result<(), SerdeError> {
        use aws_smithy_types::base64;
        self.prefix(schema);
        let encoded = base64::encode(value.as_ref());
        self.output.push('"');
        self.output.push_str(&encoded);
        self.output.push('"');
        Ok(())
    }

    fn write_timestamp(&mut self, schema: &Schema, value: &DateTime) -> Result<(), SerdeError> {
        self.prefix(schema);
        let format = self.get_timestamp_format(schema);
        let formatted = value.fmt(format).map_err(|e| SerdeError::WriteFailed {
            message: format!("failed to format timestamp: {e}"),
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

    fn write_document(&mut self, schema: &Schema, value: &Document) -> Result<(), SerdeError> {
        self.prefix(schema);
        self.write_json_value(value);
        Ok(())
    }

    fn write_null(&mut self, schema: &Schema) -> Result<(), SerdeError> {
        self.prefix(schema);
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
        let mut ser = JsonSerializer::new(Arc::new(JsonCodecSettings::default()));
        ser.write_boolean(&BOOLEAN, true).unwrap();
        let output = ser.finish();
        assert_eq!(String::from_utf8(output).unwrap(), "true");
    }

    #[test]
    fn test_write_string() {
        let mut ser = JsonSerializer::new(Arc::new(JsonCodecSettings::default()));
        ser.write_string(&STRING, "hello").unwrap();
        let output = ser.finish();
        assert_eq!(String::from_utf8(output).unwrap(), "\"hello\"");
    }

    #[test]
    fn test_write_integer() {
        let mut ser = JsonSerializer::new(Arc::new(JsonCodecSettings::default()));
        ser.write_integer(&INTEGER, 42).unwrap();
        let output = ser.finish();
        assert_eq!(String::from_utf8(output).unwrap(), "42");
    }

    #[test]
    fn test_write_null() {
        let mut ser = JsonSerializer::new(Arc::new(JsonCodecSettings::default()));
        ser.write_null(&STRING).unwrap();
        let output = ser.finish();
        assert_eq!(String::from_utf8(output).unwrap(), "null");
    }

    #[test]
    fn test_write_list() {
        let mut ser = JsonSerializer::new(Arc::new(JsonCodecSettings::default()));
        let list_schema = aws_smithy_schema::Schema::new(
            aws_smithy_schema::shape_id!("test", "List"),
            aws_smithy_schema::ShapeType::List,
            aws_smithy_schema::TraitMap::EMPTY,
        );
        ser.write_list(&list_schema, &|s: &mut dyn ShapeSerializer| {
            s.write_integer(&INTEGER, 1)?;
            s.write_integer(&INTEGER, 2)?;
            s.write_integer(&INTEGER, 3)?;
            Ok(())
        })
        .unwrap();
        let output = String::from_utf8(ser.finish()).unwrap();
        assert_eq!(output, "[1,2,3]");
    }

    #[test]
    fn test_write_full_object() {
        use aws_smithy_schema::serde::SerializableStruct;

        static ACTIVE_MEMBER: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Struct"),
            aws_smithy_schema::ShapeType::Boolean,
            aws_smithy_schema::TraitMap::EMPTY,
            "active",
            0,
        );
        static NAME_MEMBER: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Struct"),
            aws_smithy_schema::ShapeType::String,
            aws_smithy_schema::TraitMap::EMPTY,
            "name",
            1,
        );
        static COUNT_MEMBER: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Struct"),
            aws_smithy_schema::ShapeType::Integer,
            aws_smithy_schema::TraitMap::EMPTY,
            "count",
            2,
        );
        static PRICE_MEMBER: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Struct"),
            aws_smithy_schema::ShapeType::Float,
            aws_smithy_schema::TraitMap::EMPTY,
            "price",
            3,
        );
        static ITEMS_MEMBER: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Struct"),
            aws_smithy_schema::ShapeType::List,
            aws_smithy_schema::TraitMap::EMPTY,
            "items",
            4,
        );

        struct TestObject;
        impl SerializableStruct for TestObject {
            fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                s.write_boolean(&ACTIVE_MEMBER, true)?;
                s.write_string(&NAME_MEMBER, "test")?;
                s.write_integer(&COUNT_MEMBER, 42)?;
                s.write_float(&PRICE_MEMBER, 3.15)?;
                s.write_list(&ITEMS_MEMBER, &|s| {
                    s.write_integer(&INTEGER, 1)?;
                    s.write_integer(&INTEGER, 2)?;
                    Ok(())
                })?;
                Ok(())
            }
        }

        let struct_schema = aws_smithy_schema::Schema::new(
            aws_smithy_schema::shape_id!("test", "Struct"),
            aws_smithy_schema::ShapeType::Structure,
            aws_smithy_schema::TraitMap::EMPTY,
        );
        let mut ser = JsonSerializer::new(Arc::new(JsonCodecSettings::default()));
        ser.write_struct(&struct_schema, &TestObject).unwrap();
        let output = String::from_utf8(ser.finish()).unwrap();
        assert_eq!(
            output,
            r#"{"active":true,"name":"test","count":42,"price":3.15,"items":[1,2]}"#
        );
    }

    #[test]
    fn test_nested_complex_serialization() {
        use aws_smithy_schema::serde::SerializableStruct;

        // Member schemas
        static ID: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "User"),
            aws_smithy_schema::ShapeType::Long,
            aws_smithy_schema::TraitMap::EMPTY,
            "id",
            0,
        );
        static NAME: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "User"),
            aws_smithy_schema::ShapeType::String,
            aws_smithy_schema::TraitMap::EMPTY,
            "name",
            1,
        );
        static SCORES: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "User"),
            aws_smithy_schema::ShapeType::List,
            aws_smithy_schema::TraitMap::EMPTY,
            "scores",
            2,
        );
        static ADDRESS: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "User"),
            aws_smithy_schema::ShapeType::Structure,
            aws_smithy_schema::TraitMap::EMPTY,
            "address",
            3,
        );
        static COMPANIES: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "User"),
            aws_smithy_schema::ShapeType::List,
            aws_smithy_schema::TraitMap::EMPTY,
            "companies",
            4,
        );
        static TAGS: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "User"),
            aws_smithy_schema::ShapeType::Map,
            aws_smithy_schema::TraitMap::EMPTY,
            "tags",
            5,
        );
        static STREET: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Address"),
            aws_smithy_schema::ShapeType::String,
            aws_smithy_schema::TraitMap::EMPTY,
            "street",
            0,
        );
        static CITY: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Address"),
            aws_smithy_schema::ShapeType::String,
            aws_smithy_schema::TraitMap::EMPTY,
            "city",
            1,
        );
        static ZIP: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Address"),
            aws_smithy_schema::ShapeType::Integer,
            aws_smithy_schema::TraitMap::EMPTY,
            "zip",
            2,
        );
        static COMP_NAME: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Company"),
            aws_smithy_schema::ShapeType::String,
            aws_smithy_schema::TraitMap::EMPTY,
            "name",
            0,
        );
        static EMPLOYEES: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Company"),
            aws_smithy_schema::ShapeType::List,
            aws_smithy_schema::TraitMap::EMPTY,
            "employees",
            1,
        );
        static METADATA: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Company"),
            aws_smithy_schema::ShapeType::Map,
            aws_smithy_schema::TraitMap::EMPTY,
            "metadata",
            2,
        );
        static ACTIVE: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Company"),
            aws_smithy_schema::ShapeType::Boolean,
            aws_smithy_schema::TraitMap::EMPTY,
            "active",
            3,
        );
        static ROLE: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Tags"),
            aws_smithy_schema::ShapeType::String,
            aws_smithy_schema::TraitMap::EMPTY,
            "role",
            0,
        );
        static LEVEL: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Tags"),
            aws_smithy_schema::ShapeType::String,
            aws_smithy_schema::TraitMap::EMPTY,
            "level",
            1,
        );

        struct AddressStruct;
        impl SerializableStruct for AddressStruct {
            fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                s.write_string(&STREET, "123 Main St")?;
                s.write_string(&CITY, "Seattle")?;
                s.write_integer(&ZIP, 98101)?;
                Ok(())
            }
        }

        struct CompanyStruct {
            name: &'static str,
            employees: &'static [&'static str],
            metadata: &'static [(&'static str, i32)],
            active: bool,
        }
        impl SerializableStruct for CompanyStruct {
            fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                s.write_string(&COMP_NAME, self.name)?;
                s.write_list(&EMPLOYEES, &|s| {
                    for e in self.employees {
                        s.write_string(&STRING, e)?;
                    }
                    Ok(())
                })?;
                s.write_map(&METADATA, &|s| {
                    for (k, v) in self.metadata {
                        let member = Schema::new_member(
                            aws_smithy_schema::shape_id!("test", "Meta"),
                            aws_smithy_schema::ShapeType::Integer,
                            aws_smithy_schema::TraitMap::EMPTY,
                            k,
                            0,
                        );
                        s.write_integer(&member, *v)?;
                    }
                    Ok(())
                })?;
                s.write_boolean(&ACTIVE, self.active)?;
                Ok(())
            }
        }

        struct UserStruct;
        impl SerializableStruct for UserStruct {
            fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                s.write_long(&ID, 12345)?;
                s.write_string(&NAME, "John Doe")?;
                s.write_list(&SCORES, &|s| {
                    s.write_double(&DOUBLE, 95.5)?;
                    s.write_double(&DOUBLE, 87.3)?;
                    s.write_double(&DOUBLE, 92.1)?;
                    Ok(())
                })?;
                s.write_struct(&ADDRESS, &AddressStruct)?;
                s.write_list(&COMPANIES, &|s| {
                    let struct_schema = Schema::new(
                        aws_smithy_schema::shape_id!("test", "Company"),
                        aws_smithy_schema::ShapeType::Structure,
                        aws_smithy_schema::TraitMap::EMPTY,
                    );
                    s.write_struct(
                        &struct_schema,
                        &CompanyStruct {
                            name: "TechCorp",
                            employees: &["Alice", "Bob"],
                            metadata: &[("founded", 2010), ("size", 500)],
                            active: true,
                        },
                    )?;
                    s.write_struct(
                        &struct_schema,
                        &CompanyStruct {
                            name: "StartupInc",
                            employees: &["Charlie"],
                            metadata: &[("founded", 2020)],
                            active: false,
                        },
                    )?;
                    Ok(())
                })?;
                s.write_map(&TAGS, &|s| {
                    s.write_string(&ROLE, "admin")?;
                    s.write_string(&LEVEL, "senior")?;
                    Ok(())
                })?;
                Ok(())
            }
        }

        let struct_schema = Schema::new(
            aws_smithy_schema::shape_id!("test", "User"),
            aws_smithy_schema::ShapeType::Structure,
            aws_smithy_schema::TraitMap::EMPTY,
        );
        let mut ser = JsonSerializer::new(Arc::new(JsonCodecSettings::default()));
        ser.write_struct(&struct_schema, &UserStruct).unwrap();
        let output = ser.finish();
        // Expected compact JSON (br# avoids escape noise)
        let expected: &[u8] = br#"{"id":12345,"name":"John Doe","scores":[95.5,87.3,92.1],"address":{"street":"123 Main St","city":"Seattle","zip":98101},"companies":[{"name":"TechCorp","employees":["Alice","Bob"],"metadata":{"founded":2010,"size":500},"active":true},{"name":"StartupInc","employees":["Charlie"],"metadata":{"founded":2020},"active":false}],"tags":{"role":"admin","level":"senior"}}"#;
        assert_eq!(output, expected);
    }

    #[test]
    fn test_json_name_serialization() {
        use aws_smithy_schema::serde::SerializableStruct;

        // A simple string-valued trait for testing
        #[derive(Debug)]
        struct StringTrait {
            id: aws_smithy_schema::ShapeId,
            value: String,
        }
        impl aws_smithy_schema::Trait for StringTrait {
            fn trait_id(&self) -> &aws_smithy_schema::ShapeId {
                &self.id
            }
            fn as_any(&self) -> &dyn std::any::Any {
                &self.value
            }
        }

        fn json_name_traits(name: &str) -> aws_smithy_schema::TraitMap {
            let mut map = aws_smithy_schema::TraitMap::new();
            map.insert(Box::new(StringTrait {
                id: aws_smithy_schema::shape_id!("smithy.api", "jsonName"),
                value: name.to_string(),
            }));
            map
        }

        static FOO_MEMBER: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "MyStruct"),
            aws_smithy_schema::ShapeType::String,
            aws_smithy_schema::TraitMap::EMPTY,
            "foo",
            0,
        );
        // bar has @jsonName("Baz")
        // Can't use static because TraitMap with data isn't const.
        // Use a lazy static instead.
        static BAR_MEMBER: std::sync::LazyLock<Schema> = std::sync::LazyLock::new(|| {
            Schema::new_member(
                aws_smithy_schema::shape_id!("test", "MyStruct"),
                aws_smithy_schema::ShapeType::Integer,
                json_name_traits("Baz"),
                "bar",
                1,
            )
        });

        struct TestStruct;
        impl SerializableStruct for TestStruct {
            fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                s.write_string(&FOO_MEMBER, "hello")?;
                s.write_integer(&BAR_MEMBER, 42)?;
                Ok(())
            }
        }

        // With use_json_name=true (default), "bar" should serialize as "Baz"
        let struct_schema = Schema::new(
            aws_smithy_schema::shape_id!("test", "MyStruct"),
            aws_smithy_schema::ShapeType::Structure,
            aws_smithy_schema::TraitMap::EMPTY,
        );
        let mut ser = JsonSerializer::new(Arc::new(JsonCodecSettings::default()));
        ser.write_struct(&struct_schema, &TestStruct).unwrap();
        let output = String::from_utf8(ser.finish()).unwrap();
        assert_eq!(output, r#"{"foo":"hello","Baz":42}"#);

        // With use_json_name=false, "bar" should stay as "bar"
        let mut ser = JsonSerializer::new(Arc::new(
            JsonCodecSettings::builder().use_json_name(false).build(),
        ));
        ser.write_struct(&struct_schema, &TestStruct).unwrap();
        let output = String::from_utf8(ser.finish()).unwrap();
        assert_eq!(output, r#"{"foo":"hello","bar":42}"#);
    }
}
