/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! XML codec implementation for schema-based serialization.
//!
//! This module is currently a skeleton — types and trait impls compile but
//! every method body is `todo!()`. Phase 3 (`XmlSerializer`) and Phase 4
//! (`XmlDeserializer`) of the AWS REST XML schema-serde plan will fill in
//! the actual logic.

use aws_smithy_schema::codec::Codec;
use aws_smithy_types::date_time::Format as TimestampFormat;
use std::sync::Arc;

mod deserializer;
mod serializer;

pub use deserializer::XmlDeserializer;
pub use serializer::XmlSerializer;

/// Configuration for XML codec behavior.
///
/// Use the builder methods to construct settings:
/// ```
/// use aws_smithy_xml::codec::XmlCodecSettings;
///
/// let settings = XmlCodecSettings::builder().build();
/// ```
#[derive(Debug)]
pub struct XmlCodecSettings {
    default_timestamp_format: TimestampFormat,
}

impl XmlCodecSettings {
    /// Creates a builder for `XmlCodecSettings`.
    pub fn builder() -> XmlCodecSettingsBuilder {
        XmlCodecSettingsBuilder::default()
    }

    /// Default timestamp format when not specified by `@timestampFormat` trait.
    /// REST XML uses `date-time`.
    pub fn default_timestamp_format(&self) -> TimestampFormat {
        self.default_timestamp_format
    }
}

impl Default for XmlCodecSettings {
    fn default() -> Self {
        Self {
            default_timestamp_format: TimestampFormat::DateTime,
        }
    }
}

/// Builder for [`XmlCodecSettings`].
#[derive(Debug, Clone)]
pub struct XmlCodecSettingsBuilder {
    default_timestamp_format: TimestampFormat,
}

impl Default for XmlCodecSettingsBuilder {
    fn default() -> Self {
        Self {
            default_timestamp_format: TimestampFormat::DateTime,
        }
    }
}

impl XmlCodecSettingsBuilder {
    /// Default timestamp format when not specified by `@timestampFormat` trait.
    pub fn default_timestamp_format(mut self, value: TimestampFormat) -> Self {
        self.default_timestamp_format = value;
        self
    }

    /// Builds the settings.
    pub fn build(self) -> XmlCodecSettings {
        XmlCodecSettings {
            default_timestamp_format: self.default_timestamp_format,
        }
    }
}

/// XML codec for schema-based serialization and deserialization.
///
/// Used by REST XML to serialize request bodies and deserialize response
/// bodies. The codec carries no state of its own — each `create_serializer`
/// and `create_deserializer` call returns a fresh instance whose lifetime
/// brackets one (de)serialization.
///
/// # Examples
///
/// ```
/// use aws_smithy_xml::codec::{XmlCodec, XmlCodecSettings};
/// use aws_smithy_schema::codec::Codec;
///
/// let codec = XmlCodec::new(XmlCodecSettings::default());
/// let _serializer = codec.create_serializer();
/// let _deserializer = codec.create_deserializer(b"<Root/>");
/// ```
#[derive(Debug)]
pub struct XmlCodec {
    settings: Arc<XmlCodecSettings>,
}

impl XmlCodec {
    /// Creates a new XML codec with the given settings.
    pub fn new(settings: XmlCodecSettings) -> Self {
        Self {
            settings: Arc::new(settings),
        }
    }

    /// Creates a new XML codec from a pre-existing shared settings.
    pub fn from_shared_settings(settings: Arc<XmlCodecSettings>) -> Self {
        Self { settings }
    }

    /// Returns the codec settings.
    pub fn settings(&self) -> &XmlCodecSettings {
        &self.settings
    }
}

impl Default for XmlCodec {
    fn default() -> Self {
        Self::new(XmlCodecSettings::default())
    }
}

impl Codec for XmlCodec {
    type Serializer = XmlSerializer;
    type Deserializer<'a> = XmlDeserializer<'a>;

    fn create_serializer(&self) -> Self::Serializer {
        XmlSerializer::new(self.settings.clone())
    }

    fn create_deserializer<'a>(&self, input: &'a [u8]) -> Self::Deserializer<'a> {
        XmlDeserializer::new(input, self.settings.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings() {
        let settings = XmlCodecSettings::default();
        assert_eq!(
            settings.default_timestamp_format(),
            TimestampFormat::DateTime
        );
    }

    #[test]
    fn test_codec_creation() {
        let codec = XmlCodec::default();
        let _serializer = codec.create_serializer();
        let _deserializer = codec.create_deserializer(b"<Root/>");
    }

    #[test]
    fn test_end_to_end_serialize_through_codec() {
        use aws_smithy_schema::codec::FinishSerializer;
        use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
        use aws_smithy_schema::{shape_id, Schema, ShapeType};

        static NAME: Schema =
            Schema::new_member(shape_id!("test", "X$name"), ShapeType::String, "name", 0);
        static X_SCHEMA: Schema =
            Schema::new_struct(shape_id!("test", "X"), ShapeType::Structure, &[&NAME])
                .with_xml_namespace("urn:test", None);

        struct X;
        impl SerializableStruct for X {
            fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                ser.write_string(&NAME, "hello")
            }
        }

        let codec = XmlCodec::default();
        let mut ser = codec.create_serializer();
        ser.write_struct(&X_SCHEMA, &X).unwrap();
        let bytes = ser.finish();
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "<X xmlns=\"urn:test\"><name>hello</name></X>"
        );
    }

    #[test]
    fn test_end_to_end_deserialize_through_codec() {
        use aws_smithy_schema::serde::ShapeDeserializer;
        use aws_smithy_schema::{shape_id, Schema, ShapeType};

        static NAME: Schema =
            Schema::new_member(shape_id!("test", "X$name"), ShapeType::String, "name", 0);
        static AGE: Schema =
            Schema::new_member(shape_id!("test", "X$age"), ShapeType::Integer, "age", 1);
        static X_SCHEMA: Schema =
            Schema::new_struct(shape_id!("test", "X"), ShapeType::Structure, &[&NAME, &AGE]);

        let codec = XmlCodec::default();
        let xml = b"<X><name>Alice</name><age>30</age></X>";
        let mut deser = codec.create_deserializer(xml);

        let mut name = String::new();
        let mut age = 0i32;
        deser
            .read_struct(&X_SCHEMA, &mut |member, d| {
                match member.member_name().unwrap() {
                    "name" => name = d.read_string(member)?,
                    "age" => age = d.read_integer(member)?,
                    _ => {}
                }
                Ok(())
            })
            .unwrap();

        assert_eq!(name, "Alice");
        assert_eq!(age, 30);
    }
}
