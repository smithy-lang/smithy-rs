/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! JSON codec implementation for schema-based serialization.

use aws_smithy_schema::codec::Codec;
use aws_smithy_schema::document::DocumentSettings;
use aws_smithy_schema::serde::SerdeError;
use aws_smithy_schema::{shape_id, Schema, ShapeId};
use aws_smithy_types::date_time::{DateTime, Format as TimestampFormat};
use aws_smithy_types::Number;
use std::sync::Arc;

mod deserializer;
mod serializer;

pub use deserializer::JsonDeserializer;
pub use serializer::JsonSerializer;

/// Maps between Smithy member names and JSON wire field names.
///
/// When `@jsonName` is enabled, the wire name may differ from the member name.
/// This type handles the mapping in both directions and caches the reverse
/// lookup (wire name → member index) per struct schema.
#[derive(Debug)]
enum JsonFieldMapper {
    /// Uses member names directly, ignoring `@jsonName`.
    UseMemberName,
    /// Uses `@jsonName` trait values when present, falling back to member name.
    UseJsonName,
}

impl JsonFieldMapper {
    /// Returns the JSON wire name for a member schema.
    fn member_to_field<'a>(&self, member: &'a Schema) -> Option<&'a str> {
        let name = member.member_name()?;
        match self {
            JsonFieldMapper::UseMemberName => Some(name),
            JsonFieldMapper::UseJsonName => {
                if let Some(jn) = member.json_name() {
                    return Some(jn.value());
                }
                Some(name)
            }
        }
    }

    /// Resolves a JSON wire field name to a member schema within a struct schema.
    fn field_to_member<'s>(&self, schema: &'s Schema, field_name: &str) -> Option<&'s Schema> {
        match self {
            JsonFieldMapper::UseMemberName => schema.member_schema(field_name),
            JsonFieldMapper::UseJsonName => {
                // Check @jsonName on each member. For typical struct sizes
                // (< 50 members), linear scan is faster than a cached HashMap
                // behind a Mutex.
                for member in schema.members() {
                    if let Some(jn) = member.json_name() {
                        if jn.value() == field_name {
                            return Some(member);
                        }
                    } else if member.member_name() == Some(field_name) {
                        return Some(member);
                    }
                }
                None
            }
        }
    }
}

/// Configuration for JSON codec behavior.
///
/// Use the builder methods to construct settings:
/// ```
/// use aws_smithy_json::codec::JsonCodecSettings;
///
/// let settings = JsonCodecSettings::builder()
///     .use_json_name(false)
///     .build();
/// ```
///
/// # As `DocumentSettings`
///
/// `JsonCodecSettings` implements [`DocumentSettings`]. The
/// [`JsonDeserializer`] attaches `Arc<JsonCodecSettings>` to every
/// `Document` it produces so the format-aware accessors
/// ([`Document::as_blob`](aws_smithy_schema::document::Document::as_blob),
/// [`Document::as_timestamp`](aws_smithy_schema::document::Document::as_timestamp))
/// can coerce JSON-encoded blobs (base64 strings) and timestamps
/// (date-time strings or epoch-seconds numbers) back to typed Rust
/// values. The same instance is reused on the serialization side so a
/// document round-trips through `JsonDeserializer` →
/// `JsonSerializer` losslessly.
#[derive(Debug)]
pub struct JsonCodecSettings {
    field_mapper: JsonFieldMapper,
    default_timestamp_format: TimestampFormat,
    max_depth: u32,
    /// Identifies the protocol that produced this codec — used by
    /// `DocumentSettings` for diagnostics on coercion failures.
    /// Default: `aws.smithy.json#JsonCodec`. AWS protocols (awsJson1_0,
    /// awsJson1_1, restJson1) override this to their own shape id.
    protocol_id: ShapeId,
}

impl JsonCodecSettings {
    /// Creates a builder for `JsonCodecSettings`.
    pub fn builder() -> JsonCodecSettingsBuilder {
        JsonCodecSettingsBuilder::default()
    }

    /// Default timestamp format when not specified by `@timestampFormat` trait.
    pub fn default_timestamp_format(&self) -> TimestampFormat {
        self.default_timestamp_format
    }

    /// Maximum aggregate nesting depth the deserializer will accept before
    /// returning an error. Defends against stack overflow on recursive shapes
    /// and deeply-nested document payloads.
    pub fn max_depth(&self) -> u32 {
        self.max_depth
    }

    /// Returns the JSON wire name for a member schema.
    pub(crate) fn member_to_field<'a>(&self, member: &'a Schema) -> Option<&'a str> {
        self.field_mapper.member_to_field(member)
    }

    /// Resolves a JSON wire field name to a member schema.
    pub(crate) fn field_to_member<'s>(
        &self,
        schema: &'s Schema,
        field_name: &str,
    ) -> Option<&'s Schema> {
        self.field_mapper.field_to_member(schema, field_name)
    }
}

impl Default for JsonCodecSettings {
    fn default() -> Self {
        Self {
            field_mapper: JsonFieldMapper::UseJsonName,
            default_timestamp_format: TimestampFormat::EpochSeconds,
            max_depth: crate::codec::deserializer::MAX_DESERIALIZE_DEPTH,
            protocol_id: DEFAULT_JSON_CODEC_ID,
        }
    }
}

/// Sentinel protocol id used by `JsonCodecSettings` when none is
/// supplied — surfaces in `DocumentSettings` diagnostics. AWS
/// protocols override this with their own ids.
const DEFAULT_JSON_CODEC_ID: ShapeId = shape_id!("aws.smithy.json", "JsonCodec");

/// Builder for [`JsonCodecSettings`].
#[derive(Debug, Clone)]
pub struct JsonCodecSettingsBuilder {
    use_json_name: bool,
    default_timestamp_format: TimestampFormat,
    max_depth: u32,
    protocol_id: ShapeId,
}

impl Default for JsonCodecSettingsBuilder {
    fn default() -> Self {
        Self {
            use_json_name: true,
            default_timestamp_format: TimestampFormat::EpochSeconds,
            max_depth: crate::codec::deserializer::MAX_DESERIALIZE_DEPTH,
            protocol_id: DEFAULT_JSON_CODEC_ID,
        }
    }
}

impl JsonCodecSettingsBuilder {
    /// Whether to use the `@jsonName` trait for member names.
    pub fn use_json_name(mut self, value: bool) -> Self {
        self.use_json_name = value;
        self
    }

    /// Default timestamp format when not specified by `@timestampFormat` trait.
    pub fn default_timestamp_format(mut self, value: TimestampFormat) -> Self {
        self.default_timestamp_format = value;
        self
    }

    /// Sets the maximum aggregate nesting depth the deserializer will accept
    /// before returning an error. Defaults to 128.
    pub fn max_depth(mut self, value: u32) -> Self {
        self.max_depth = value;
        self
    }

    /// Sets the protocol id surfaced in `DocumentSettings`
    /// diagnostics. Defaults to `aws.smithy.json#JsonCodec`. AWS
    /// protocols (awsJson1_0, awsJson1_1, restJson1) supply their own
    /// shape id so coercion errors point at the originating protocol.
    pub fn protocol_id(mut self, value: ShapeId) -> Self {
        self.protocol_id = value;
        self
    }

    /// Builds the settings.
    pub fn build(self) -> JsonCodecSettings {
        let field_mapper = if self.use_json_name {
            JsonFieldMapper::UseJsonName
        } else {
            JsonFieldMapper::UseMemberName
        };
        JsonCodecSettings {
            field_mapper,
            default_timestamp_format: self.default_timestamp_format,
            max_depth: self.max_depth,
            protocol_id: self.protocol_id,
        }
    }
}

impl DocumentSettings for JsonCodecSettings {
    fn protocol_id(&self) -> &ShapeId {
        &self.protocol_id
    }

    /// Resolves a JSON wire field name to the index of the matching
    /// member, honoring `@jsonName` when configured via
    /// [`JsonCodecSettingsBuilder::use_json_name`].
    ///
    /// Falls back to `None` if no member matches — matching the JSON
    /// "ignore unknown fields" convention.
    fn member_index_for(&self, schema: &Schema, wire_name: &str) -> Option<usize> {
        // `field_to_member` honors @jsonName per `JsonFieldMapper`.
        // `Schema::member_index` is `Option<usize>`, returning `Some`
        // for member schemas (which is what `field_to_member` always
        // returns).
        self.field_to_member(schema, wire_name)?.member_index()
    }

    /// Decodes a base64 string into a blob.
    ///
    /// JSON has no native blob type; the
    /// [Smithy spec](https://smithy.io/2.0/spec/simple-types.html#blob)
    /// requires blobs to be transmitted as base64-encoded strings on
    /// the JSON wire. This method consumes those strings on the
    /// deserialization side; the corresponding encode happens in
    /// `JsonSerializer::write_document`.
    fn coerce_string_to_blob(&self, s: &str) -> Result<Vec<u8>, SerdeError> {
        aws_smithy_types::base64::decode(s).map_err(|e| SerdeError::BlobDecodeFailed {
            message: e.to_string(),
        })
    }

    /// Parses a string-formatted timestamp using the codec's default
    /// timestamp format ([`JsonCodecSettings::default_timestamp_format`]).
    ///
    /// Per the SEP, untyped JSON documents have no schema-level
    /// `@timestampFormat` trait to consult, so the codec's default
    /// format is the only signal. AWS JSON / restJson1 default to
    /// `epoch-seconds` — but a string-typed JSON value cannot be
    /// epoch-seconds (which is a number). For this case we attempt
    /// `date-time` parsing as a fallback so common ISO-8601 strings
    /// still coerce.
    fn coerce_string_to_timestamp(&self, s: &str) -> Result<DateTime, SerdeError> {
        let primary_format = self.default_timestamp_format;
        // If the configured default is a string-encoded format, use it.
        let attempt = match primary_format {
            TimestampFormat::DateTime | TimestampFormat::HttpDate => {
                DateTime::from_str(s, primary_format)
            }
            // Number-encoded format: a string value can't satisfy it.
            // Fall back to `date-time` as the most common JSON
            // string-timestamp encoding.
            TimestampFormat::EpochSeconds => DateTime::from_str(s, TimestampFormat::DateTime),
            _ => DateTime::from_str(s, TimestampFormat::DateTime),
        };
        attempt.map_err(|e| SerdeError::TimestampParseFailed {
            message: e.to_string(),
        })
    }

    /// Coerces a number to a timestamp interpreted as
    /// `epoch-seconds`.
    ///
    /// Number-typed JSON values are only valid timestamps when the
    /// codec's default format is `epoch-seconds`. Per the SEP this is
    /// the AWS JSON / restJson1 default. If the codec has been
    /// configured with a non-numeric default format
    /// (`date-time` / `http-date`), this method returns an
    /// `UnsupportedOperation` error — a number value isn't valid for
    /// those formats.
    fn coerce_number_to_timestamp(&self, n: &Number) -> Result<DateTime, SerdeError> {
        if !matches!(self.default_timestamp_format, TimestampFormat::EpochSeconds) {
            return Err(SerdeError::UnsupportedOperation {
                message: format!(
                    "JSON codec configured with timestamp format {:?}; \
                     number-to-timestamp coercion only valid for epoch-seconds",
                    self.default_timestamp_format
                ),
            });
        }
        Ok(match *n {
            Number::PosInt(u) => {
                if u > i64::MAX as u64 {
                    return Err(SerdeError::TimestampParseFailed {
                        message: format!(
                            "epoch-seconds value {u} overflows i64; cannot construct DateTime"
                        ),
                    });
                }
                DateTime::from_secs(u as i64)
            }
            Number::NegInt(i) => DateTime::from_secs(i),
            Number::Float(f) => {
                if !f.is_finite() {
                    return Err(SerdeError::TimestampParseFailed {
                        message: format!(
                            "non-finite epoch-seconds value {f}; cannot construct DateTime"
                        ),
                    });
                }
                DateTime::from_secs_f64(f)
            }
        })
    }
}

/// JSON codec for schema-based serialization and deserialization.
///
/// # Examples
///
/// ```
/// use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
/// use aws_smithy_schema::codec::Codec;
///
/// // Create codec with default settings (REST JSON style)
/// let codec = JsonCodec::new(JsonCodecSettings::default());
///
/// // Create codec for AWS JSON RPC (no jsonName, epoch-seconds timestamps)
/// let codec = JsonCodec::new(
///     JsonCodecSettings::builder()
///         .use_json_name(false)
///         .build()
/// );
/// ```
#[derive(Debug)]
pub struct JsonCodec {
    settings: Arc<JsonCodecSettings>,
}

impl JsonCodec {
    /// Creates a new JSON codec with the given settings.
    pub fn new(settings: JsonCodecSettings) -> Self {
        Self {
            settings: Arc::new(settings),
        }
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
    type Deserializer<'a> = JsonDeserializer<'a>;

    fn create_serializer(&self) -> Self::Serializer {
        JsonSerializer::new(self.settings.clone())
    }

    fn create_deserializer<'a>(&self, input: &'a [u8]) -> Self::Deserializer<'a> {
        JsonDeserializer::new(input, self.settings.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings() {
        let settings = JsonCodecSettings::default();
        assert_eq!(
            settings.default_timestamp_format(),
            TimestampFormat::EpochSeconds
        );
    }

    #[test]
    fn test_builder() {
        let settings = JsonCodecSettings::builder()
            .use_json_name(false)
            .default_timestamp_format(TimestampFormat::DateTime)
            .build();
        assert_eq!(
            settings.default_timestamp_format(),
            TimestampFormat::DateTime
        );
    }

    #[test]
    fn test_codec_creation() {
        let codec = JsonCodec::default();
        let _serializer = codec.create_serializer();
        let _deserializer = codec.create_deserializer(b"{}");
    }

    #[test]
    fn document_settings_protocol_id_default_is_codec_sentinel() {
        let settings = JsonCodecSettings::default();
        assert_eq!(
            DocumentSettings::protocol_id(&settings).as_str(),
            "aws.smithy.json#JsonCodec"
        );
    }

    #[test]
    fn document_settings_protocol_id_can_be_overridden() {
        let settings = JsonCodecSettings::builder()
            .protocol_id(shape_id!("aws.protocols", "restJson1"))
            .build();
        assert_eq!(
            DocumentSettings::protocol_id(&settings).as_str(),
            "aws.protocols#restJson1"
        );
    }

    #[test]
    fn document_settings_member_index_for_resolves_member_name() {
        use aws_smithy_schema::ShapeType;

        // Build a tiny struct schema with two members.
        static M_FIRST: Schema = Schema::new_member(
            shape_id!("com.example", "MyStruct", "first"),
            ShapeType::String,
            "first",
            0,
        );
        static M_SECOND: Schema = Schema::new_member(
            shape_id!("com.example", "MyStruct", "second"),
            ShapeType::Integer,
            "second",
            1,
        );
        static MY_STRUCT: Schema = Schema::new_struct(
            shape_id!("com.example", "MyStruct"),
            ShapeType::Structure,
            &[&M_FIRST, &M_SECOND],
        );

        let settings = JsonCodecSettings::builder().use_json_name(false).build();
        assert_eq!(settings.member_index_for(&MY_STRUCT, "first"), Some(0));
        assert_eq!(settings.member_index_for(&MY_STRUCT, "second"), Some(1));
        assert_eq!(settings.member_index_for(&MY_STRUCT, "missing"), None);
    }

    #[test]
    fn document_settings_member_index_for_honors_json_name() {
        use aws_smithy_schema::ShapeType;

        // Member's wire name is "first_json" via @jsonName.
        static M_FIRST: Schema = Schema::new_member(
            shape_id!("com.example", "MyStruct", "first"),
            ShapeType::String,
            "first",
            0,
        )
        .with_json_name("first_json");
        static MY_STRUCT: Schema = Schema::new_struct(
            shape_id!("com.example", "MyStruct"),
            ShapeType::Structure,
            &[&M_FIRST],
        );

        let settings = JsonCodecSettings::builder().use_json_name(true).build();
        // With use_json_name=true: lookup by wire name succeeds, lookup
        // by member name fails (because @jsonName takes precedence).
        assert_eq!(settings.member_index_for(&MY_STRUCT, "first_json"), Some(0));
        assert_eq!(settings.member_index_for(&MY_STRUCT, "first"), None);
    }

    #[test]
    fn document_settings_coerce_string_to_blob_decodes_base64() {
        let settings = JsonCodecSettings::default();
        // base64 of "hello"
        let bytes = settings.coerce_string_to_blob("aGVsbG8=").unwrap();
        assert_eq!(bytes, b"hello");
    }

    #[test]
    fn document_settings_coerce_string_to_blob_returns_error_on_invalid() {
        let settings = JsonCodecSettings::default();
        let err = settings
            .coerce_string_to_blob("not!valid!base64!")
            .unwrap_err();
        match err {
            SerdeError::BlobDecodeFailed { .. } => {}
            other => panic!("expected BlobDecodeFailed, got {other:?}"),
        }
    }

    #[test]
    fn document_settings_coerce_string_to_timestamp_with_date_time_default() {
        let settings = JsonCodecSettings::builder()
            .default_timestamp_format(TimestampFormat::DateTime)
            .build();
        let dt = settings
            .coerce_string_to_timestamp("1970-01-01T00:00:00Z")
            .unwrap();
        assert_eq!(dt.secs(), 0);
    }

    #[test]
    fn document_settings_coerce_string_to_timestamp_falls_back_to_date_time_for_epoch_default() {
        // Default format is EpochSeconds (numeric); a string can't
        // satisfy that, so the impl falls back to date-time parsing.
        let settings = JsonCodecSettings::default();
        let dt = settings
            .coerce_string_to_timestamp("1970-01-01T00:00:00Z")
            .unwrap();
        assert_eq!(dt.secs(), 0);
    }

    #[test]
    fn document_settings_coerce_string_to_timestamp_error() {
        let settings = JsonCodecSettings::default();
        let err = settings
            .coerce_string_to_timestamp("not a timestamp")
            .unwrap_err();
        match err {
            SerdeError::TimestampParseFailed { .. } => {}
            other => panic!("expected TimestampParseFailed, got {other:?}"),
        }
    }

    #[test]
    fn document_settings_coerce_number_to_timestamp_epoch_seconds() {
        let settings = JsonCodecSettings::default();
        // PosInt
        let dt = settings
            .coerce_number_to_timestamp(&Number::PosInt(1_700_000_000))
            .unwrap();
        assert_eq!(dt.secs(), 1_700_000_000);
        // NegInt (pre-epoch)
        let dt = settings
            .coerce_number_to_timestamp(&Number::NegInt(-100))
            .unwrap();
        assert_eq!(dt.secs(), -100);
        // Float (fractional seconds)
        let dt = settings
            .coerce_number_to_timestamp(&Number::Float(1.5))
            .unwrap();
        assert_eq!(dt.secs(), 1);
        assert_eq!(dt.subsec_nanos(), 500_000_000);
    }

    #[test]
    fn document_settings_coerce_number_to_timestamp_unsupported_for_string_format() {
        let settings = JsonCodecSettings::builder()
            .default_timestamp_format(TimestampFormat::DateTime)
            .build();
        let err = settings
            .coerce_number_to_timestamp(&Number::PosInt(0))
            .unwrap_err();
        match err {
            SerdeError::UnsupportedOperation { .. } => {}
            other => panic!("expected UnsupportedOperation, got {other:?}"),
        }
    }

    #[test]
    fn document_settings_coerce_number_to_timestamp_rejects_non_finite_float() {
        let settings = JsonCodecSettings::default();
        let err = settings
            .coerce_number_to_timestamp(&Number::Float(f64::NAN))
            .unwrap_err();
        match err {
            SerdeError::TimestampParseFailed { .. } => {}
            other => panic!("expected TimestampParseFailed, got {other:?}"),
        }
    }
}
