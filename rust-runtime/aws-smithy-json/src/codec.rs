/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! JSON codec implementation for schema-based serialization.

use aws_smithy_schema::codec::Codec;
use aws_smithy_types::date_time::Format as TimestampFormat;

mod deserializer;
mod serializer;

pub use deserializer::JsonDeserializer;
pub use serializer::JsonSerializer;

/// Configuration for JSON codec behavior.
#[derive(Debug, Clone)]
pub struct JsonCodecSettings {
    /// Whether to use the @jsonName trait for member names.
    pub use_json_name: bool,
    /// Default timestamp format to use when not specified by @timestampFormat trait.
    pub default_timestamp_format: TimestampFormat,
    /// Whether to allow unknown union members during deserialization.
    pub allow_unknown_union_members: bool,
}

impl Default for JsonCodecSettings {
    fn default() -> Self {
        Self {
            use_json_name: true,
            default_timestamp_format: TimestampFormat::EpochSeconds,
            allow_unknown_union_members: false,
        }
    }
}

/// JSON codec for schema-based serialization and deserialization.
///
/// This codec implements the Smithy JSON protocol serialization rules,
/// with configurable behavior for different protocol variants (e.g., AWS JSON RPC vs REST JSON).
///
/// # Examples
///
/// ```
/// use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
/// use aws_smithy_schema::codec::Codec;
/// use aws_smithy_types::date_time::Format;
///
/// // Create codec with default settings (REST JSON style)
/// let codec = JsonCodec::new(JsonCodecSettings::default());
///
/// // Create codec for AWS JSON RPC (no jsonName, epoch-seconds timestamps)
/// let codec = JsonCodec::new(JsonCodecSettings {
///     use_json_name: false,
///     default_timestamp_format: Format::EpochSeconds,
///     allow_unknown_union_members: false,
/// });
///
/// // Use the codec
/// let _serializer = codec.create_serializer();
/// let _deserializer = codec.create_deserializer(b"{}");
/// ```
#[derive(Debug, Clone)]
pub struct JsonCodec {
    settings: JsonCodecSettings,
}

impl JsonCodec {
    /// Creates a new JSON codec with the given settings.
    pub fn new(settings: JsonCodecSettings) -> Self {
        Self { settings }
    }

    /// Returns the codec settings.
    pub fn settings(&self) -> &JsonCodecSettings {
        &self.settings
    }
}

impl Default for JsonCodec {
    fn default() -> Self {
        Self::new(JsonCodecSettings::default())
    }
}

impl Codec for JsonCodec {
    type Serializer = JsonSerializer;
    type Deserializer = JsonDeserializer;

    fn create_serializer(&self) -> Self::Serializer {
        JsonSerializer::new(self.settings.clone())
    }

    fn create_deserializer(&self, input: &[u8]) -> Self::Deserializer {
        JsonDeserializer::new(input, self.settings.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings() {
        let settings = JsonCodecSettings::default();
        assert!(settings.use_json_name);
        assert_eq!(
            settings.default_timestamp_format,
            TimestampFormat::EpochSeconds
        );
        assert!(!settings.allow_unknown_union_members);
    }

    #[test]
    fn test_codec_creation() {
        let codec = JsonCodec::default();
        let _serializer = codec.create_serializer();
        let _deserializer = codec.create_deserializer(b"{}");
    }
}
