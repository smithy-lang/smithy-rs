/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The server-side protocol trait: schema-driven output and error serialization.
//!
//! [`ServerProtocol`] is implemented once per protocol on the existing
//! zero-sized protocol markers ([`RestJson1`], [`AwsJson1_0`], [`AwsJson1_1`],
//! [`RpcV2Cbor`], [`RestXml`]). It owns the schema-driven body codec and the
//! `serialize_output` / `serialize_error` seam. A single trait — deliberately
//! not mirroring the client's `ClientProtocolInner` / object-safe
//! `ClientProtocol` pair — because server protocol dispatch is fully static:
//! protocols come from the statically nested multi-protocol router, never from
//! config.
//!
//! # Error framing (frozen to legacy generated behavior)
//!
//! Wire discriminators are derived from the error shape's own full `ShapeId`
//! and emitted per-protocol exactly as today's generated serializers do:
//!
//! | Protocol   | Discriminator                                                  |
//! |------------|----------------------------------------------------------------|
//! | restJson1  | `x-amzn-errortype` header, shape **name only**; none in body   |
//! | awsJson1.0 | `__type` body member, **full** `namespace#Name`, written last  |
//! | awsJson1.1 | `__type` body member, shape **name only**, written last        |
//! | rpcv2Cbor  | `__type` body member, **full** `namespace#Name`, written first |
//! | restXml    | none                                                           |
//!
//! `@httpHeader`-bound error members are split out of the body and stamped as
//! response headers on the REST protocols, mirroring the legacy generated
//! `ser_*_headers` functions (including the skip-empty-string rule).
//!
//! Serializers never detect errors; call sites declare them: `is_error`
//! selects the error framing, and [`ServerProtocol::serialize_error`] is the
//! declaration on the error path.

use std::cell::RefCell;
use std::sync::LazyLock;

use aws_smithy_schema::codec::{Codec, FinishSerializer};
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::{Schema, ShapeId, ShapeType};
use aws_smithy_types::{BigDecimal, BigInteger, DateTime, Document};

use crate::body::BoxBody;
use crate::extension::ModeledErrorExtension;
use crate::modeled_error::HttpModeledError;
use crate::response::IntoResponse;

use super::aws_json_10::AwsJson1_0;
use super::aws_json_11::AwsJson1_1;
use super::rest_json_1::RestJson1;
use super::rest_xml::RestXml;
use super::rpc_v2_cbor::RpcV2Cbor;
use super::ProtocolShape;

use aws_smithy_cbor::codec::{CborCodec, CborCodecSettings};
use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
use aws_smithy_xml::codec::{XmlCodec, XmlCodecSettings};

/// Implemented on each protocol marker. One impl per protocol; all dispatch is
/// static — the multi-protocol router nests protocol services that are each
/// monomorphized over their marker, so by the time output or an error is
/// serialized the protocol is statically known.
pub trait ServerProtocol: ProtocolShape {
    /// The schema-driven body codec for this protocol (e.g. `JsonCodec`
    /// configured for restJson1).
    ///
    /// Associated type, not `DynCodec`: `FinishSerializer::finish` is not
    /// object-safe, and no protocol-erased call site exists server-side.
    type Codec: Codec;

    /// Returns this protocol's codec.
    fn codec(&self) -> &Self::Codec;

    /// Serializes a success or error payload to a complete response with the
    /// protocol content-type and content-length stamped and status `200`.
    ///
    /// `is_error` selects error framing (discriminator injection,
    /// `@httpHeader` member splitting); serializers never detect errors —
    /// call sites declare them.
    fn serialize_output(
        &self,
        schema: &Schema<'_>,
        output: &dyn SerializableStruct,
        is_error: bool,
    ) -> Result<http::Response<BoxBody>, SerdeError>;

    /// Serializes a modeled error to a complete response: status from
    /// [`HttpModeledError::status_code`], protocol discriminator,
    /// content-type, and body via
    /// [`serialize_output`](ServerProtocol::serialize_output) with
    /// `is_error = true`.
    ///
    /// Serialization failure logs via `tracing` and falls back to the
    /// protocol's `RuntimeError::Serialization` response, preserving the
    /// legacy generated `IntoResponse` fallback semantics.
    fn serialize_error<E: HttpModeledError + ?Sized>(&self, error: &E) -> http::Response<BoxBody>;
}

/// Sized adapter so a `&E` with `E: SerializableStruct + ?Sized` (e.g.
/// `&dyn HttpModeledError`) can be passed where `&dyn SerializableStruct` is
/// required — unsized-to-`dyn` coercion needs a sized source.
struct AsSerializable<'a, E: ?Sized>(&'a E);

impl<E: SerializableStruct + ?Sized> SerializableStruct for AsSerializable<'_, E> {
    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        self.0.serialize_members(serializer)
    }
}

// ============================================================================
// Discriminator injection
// ============================================================================

/// Member schema for the synthetic `__type` discriminator member.
///
/// The member index is irrelevant on the serialization path (codecs key off
/// `member_name`); `usize::MAX` guards against accidental use for
/// deserialization-side member lookup.
static TYPE_MEMBER: Schema<'static> = Schema::new_member(
    ShapeId::from_parts("smithy.api#String", "smithy.api", "String"),
    ShapeType::String,
    "__type",
    usize::MAX,
);

/// Wrapper prepending a synthetic `__type` member before the inner shape's
/// members (rpcv2Cbor: `__type` is the first map entry).
struct WithTypeFirst<'a> {
    type_value: &'a str,
    inner: &'a dyn SerializableStruct,
}

impl SerializableStruct for WithTypeFirst<'_> {
    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        serializer.write_string(&TYPE_MEMBER, self.type_value)?;
        self.inner.serialize_members(serializer)
    }
}

/// Wrapper appending a synthetic `__type` member after the inner shape's
/// members (awsJson 1.0 / 1.1: `__type` is written last, matching the legacy
/// generated serializers).
struct WithTypeLast<'a> {
    type_value: &'a str,
    inner: &'a dyn SerializableStruct,
}

impl SerializableStruct for WithTypeLast<'_> {
    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        self.inner.serialize_members(serializer)?;
        serializer.write_string(&TYPE_MEMBER, self.type_value)
    }
}

// ============================================================================
// `@httpHeader` member splitting (REST protocols)
// ============================================================================

type CapturedHeaders = RefCell<Vec<(http::HeaderName, http::HeaderValue)>>;

/// Returns `true` if any top-level member of `schema` is `@httpHeader`-bound.
fn has_header_bound_members(schema: &Schema<'_>) -> bool {
    schema.members().iter().any(|m| m.http_header().is_some())
}

/// Wrapper that diverts `@httpHeader`-bound top-level members into a header
/// sink while forwarding everything else to the body serializer.
struct SplitHttpHeaders<'a> {
    inner: &'a dyn SerializableStruct,
    sink: &'a CapturedHeaders,
}

impl SerializableStruct for SplitHttpHeaders<'_> {
    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        let mut splitter = HeaderSplitter {
            inner: serializer,
            sink: self.sink,
        };
        self.inner.serialize_members(&mut splitter)
    }
}

fn capture_header(
    sink: &CapturedHeaders,
    schema: &Schema<'_>,
    formatted: &str,
) -> Result<(), SerdeError> {
    // Mirror the legacy generated `ser_*_headers` functions: empty string
    // values are skipped rather than sent as empty headers.
    if formatted.is_empty() {
        return Ok(());
    }
    let header = schema
        .http_header()
        .expect("checked by caller: schema carries @httpHeader");
    let name = http::HeaderName::try_from(header.value()).map_err(|err| {
        SerdeError::custom(format!(
            "`{}` cannot be used as a header name: {}",
            header.value(),
            err
        ))
    })?;
    let value = http::HeaderValue::try_from(formatted).map_err(|err| {
        SerdeError::custom(format!("`{formatted}` cannot be used as a header value: {err}"))
    })?;
    sink.borrow_mut().push((name, value));
    Ok(())
}

/// Formats a timestamp for an HTTP header: `@timestampFormat` if present on
/// the member schema, else `http-date` (the Smithy default for header-bound
/// timestamps).
fn format_header_timestamp(schema: &Schema<'_>, value: &DateTime) -> Result<String, SerdeError> {
    use aws_smithy_schema::traits::TimestampFormat as SchemaFormat;
    use aws_smithy_types::date_time::Format;
    let format = match schema.timestamp_format().map(|t| t.format()) {
        Some(SchemaFormat::EpochSeconds) => Format::EpochSeconds,
        Some(SchemaFormat::DateTime) => Format::DateTimeWithOffset,
        Some(SchemaFormat::HttpDate) | None => Format::HttpDate,
    };
    value
        .fmt(format)
        .map_err(|err| SerdeError::custom(format!("failed to format timestamp header: {err}")))
    }

/// Serializer that intercepts `@httpHeader`-bound member writes and forwards
/// the rest to the wrapped body serializer.
struct HeaderSplitter<'a> {
    inner: &'a mut dyn ShapeSerializer,
    sink: &'a CapturedHeaders,
}

impl HeaderSplitter<'_> {
    fn is_header(&self, schema: &Schema<'_>) -> bool {
        schema.http_header().is_some()
    }
}

macro_rules! split_scalar {
    ($fn_name:ident, $ty:ty) => {
        fn $fn_name(&mut self, schema: &Schema<'_>, value: $ty) -> Result<(), SerdeError> {
            if self.is_header(schema) {
                let mut encoder = aws_smithy_types::primitive::Encoder::from(value);
                capture_header(self.sink, schema, encoder.encode())
            } else {
                self.inner.$fn_name(schema, value)
            }
        }
    };
}

impl ShapeSerializer for HeaderSplitter<'_> {
    fn write_struct(
        &mut self,
        schema: &Schema<'_>,
        value: &dyn SerializableStruct,
    ) -> Result<(), SerdeError> {
        // `@httpHeader` cannot target structures; always a body member.
        self.inner.write_struct(schema, value)
    }

    fn write_list(
        &mut self,
        schema: &Schema<'_>,
        write_elements: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        if self.is_header(schema) {
            // Each element becomes its own header value under the same name;
            // the collector formats elements against the outer
            // (header-carrying) member schema.
            let mut collector = HeaderListCollector {
                sink: self.sink,
                outer: schema,
            };
            write_elements(&mut collector)
        } else {
            self.inner.write_list(schema, write_elements)
        }
    }

    fn write_map(
        &mut self,
        schema: &Schema<'_>,
        write_entries: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        // `@httpHeader` cannot target maps (`@httpPrefixHeaders` is a
        // different binding, out of scope for error responses).
        self.inner.write_map(schema, write_entries)
    }

    split_scalar!(write_boolean, bool);
    split_scalar!(write_byte, i8);
    split_scalar!(write_short, i16);
    split_scalar!(write_integer, i32);
    split_scalar!(write_long, i64);
    split_scalar!(write_float, f32);
    split_scalar!(write_double, f64);

    fn write_big_integer(
        &mut self,
        schema: &Schema<'_>,
        value: &BigInteger,
    ) -> Result<(), SerdeError> {
        if self.is_header(schema) {
            capture_header(self.sink, schema, value.as_ref())
        } else {
            self.inner.write_big_integer(schema, value)
        }
    }

    fn write_big_decimal(
        &mut self,
        schema: &Schema<'_>,
        value: &BigDecimal,
    ) -> Result<(), SerdeError> {
        if self.is_header(schema) {
            capture_header(self.sink, schema, value.as_ref())
        } else {
            self.inner.write_big_decimal(schema, value)
        }
    }

    fn write_string(&mut self, schema: &Schema<'_>, value: &str) -> Result<(), SerdeError> {
        if self.is_header(schema) {
            capture_header(self.sink, schema, value)
        } else {
            self.inner.write_string(schema, value)
        }
    }

    fn write_blob(&mut self, schema: &Schema<'_>, value: &[u8]) -> Result<(), SerdeError> {
        if self.is_header(schema) {
            capture_header(self.sink, schema, &aws_smithy_types::base64::encode(value))
        } else {
            self.inner.write_blob(schema, value)
        }
    }

    fn write_timestamp(&mut self, schema: &Schema<'_>, value: &DateTime) -> Result<(), SerdeError> {
        if self.is_header(schema) {
            let formatted = format_header_timestamp(schema, value)?;
            capture_header(self.sink, schema, &formatted)
        } else {
            self.inner.write_timestamp(schema, value)
        }
    }

    fn write_document(&mut self, schema: &Schema<'_>, value: &Document) -> Result<(), SerdeError> {
        // `@httpHeader` cannot target documents; always a body member.
        self.inner.write_document(schema, value)
    }

    fn write_null(&mut self, schema: &Schema<'_>) -> Result<(), SerdeError> {
        if self.is_header(schema) {
            // A null header-bound member is simply not sent.
            Ok(())
        } else {
            self.inner.write_null(schema)
        }
    }
}

/// Collects the elements of an `@httpHeader`-bound list member: each element
/// becomes its own header value under the member's header name.
struct HeaderListCollector<'a> {
    sink: &'a CapturedHeaders,
    /// The header-carrying member schema.
    outer: &'a Schema<'a>,
}

macro_rules! collect_scalar {
    ($fn_name:ident, $ty:ty) => {
        fn $fn_name(&mut self, _schema: &Schema<'_>, value: $ty) -> Result<(), SerdeError> {
            let mut encoder = aws_smithy_types::primitive::Encoder::from(value);
            capture_header(self.sink, self.outer, encoder.encode())
        }
    };
}

impl ShapeSerializer for HeaderListCollector<'_> {
    fn write_struct(
        &mut self,
        _schema: &Schema<'_>,
        _value: &dyn SerializableStruct,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::custom(
            "structures cannot appear in an @httpHeader-bound list",
        ))
    }

    fn write_list(
        &mut self,
        _schema: &Schema<'_>,
        _write_elements: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::custom(
            "nested lists cannot appear in an @httpHeader-bound list",
        ))
    }

    fn write_map(
        &mut self,
        _schema: &Schema<'_>,
        _write_entries: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::custom(
            "maps cannot appear in an @httpHeader-bound list",
        ))
    }

    collect_scalar!(write_boolean, bool);
    collect_scalar!(write_byte, i8);
    collect_scalar!(write_short, i16);
    collect_scalar!(write_integer, i32);
    collect_scalar!(write_long, i64);
    collect_scalar!(write_float, f32);
    collect_scalar!(write_double, f64);

    fn write_big_integer(
        &mut self,
        _schema: &Schema<'_>,
        value: &BigInteger,
    ) -> Result<(), SerdeError> {
        capture_header(self.sink, self.outer, value.as_ref())
    }

    fn write_big_decimal(
        &mut self,
        _schema: &Schema<'_>,
        value: &BigDecimal,
    ) -> Result<(), SerdeError> {
        capture_header(self.sink, self.outer, value.as_ref())
    }

    fn write_string(&mut self, _schema: &Schema<'_>, value: &str) -> Result<(), SerdeError> {
        capture_header(self.sink, self.outer, value)
    }

    fn write_blob(&mut self, _schema: &Schema<'_>, value: &[u8]) -> Result<(), SerdeError> {
        capture_header(self.sink, self.outer, &aws_smithy_types::base64::encode(value))
    }

    fn write_timestamp(&mut self, _schema: &Schema<'_>, value: &DateTime) -> Result<(), SerdeError> {
        let formatted = format_header_timestamp(self.outer, value)?;
        capture_header(self.sink, self.outer, &formatted)
    }

    fn write_document(&mut self, _schema: &Schema<'_>, _value: &Document) -> Result<(), SerdeError> {
        Err(SerdeError::custom(
            "documents cannot appear in an @httpHeader-bound list",
        ))
    }

    fn write_null(&mut self, _schema: &Schema<'_>) -> Result<(), SerdeError> {
        // Sparse list null elements are not representable in headers; skip.
        Ok(())
    }
}

// ============================================================================
// Shared response assembly
// ============================================================================

/// Serializes `value` through `codec`, optionally splitting
/// `@httpHeader`-bound members out of the body.
fn serialize_body<C: Codec>(
    codec: &C,
    schema: &Schema<'_>,
    value: &dyn SerializableStruct,
    split_headers: bool,
) -> Result<(Vec<u8>, Vec<(http::HeaderName, http::HeaderValue)>), SerdeError> {
    let mut serializer = codec.create_serializer();
    if split_headers && has_header_bound_members(schema) {
        let sink = CapturedHeaders::default();
        let wrapper = SplitHttpHeaders { inner: value, sink: &sink };
        serializer.write_struct(schema, &wrapper)?;
        Ok((serializer.finish(), sink.into_inner()))
    } else {
        serializer.write_struct(schema, value)?;
        Ok((serializer.finish(), Vec::new()))
    }
}

/// Assembles the response: content-type, captured `@httpHeader` values,
/// content-length, status 200 (the caller overrides on the error path).
///
/// Mirrors the legacy generated `ser_*_http_error` functions, which stamp
/// content-type, protocol-specific headers, and content-length via
/// `set_response_header_if_absent` (the headers cannot already be present on
/// a fresh builder, so plain insertion is equivalent).
fn assemble_response(
    body: Vec<u8>,
    content_type: &'static str,
    extra_headers: Vec<(http::HeaderName, http::HeaderValue)>,
) -> Result<http::Response<BoxBody>, SerdeError> {
    let mut builder = http::Response::builder()
        .status(http::StatusCode::OK)
        .header(http::header::CONTENT_TYPE, content_type);
    for (name, value) in extra_headers {
        builder = builder.header(name, value);
    }
    builder = builder.header(http::header::CONTENT_LENGTH, body.len());
    builder
        .body(crate::body::to_boxed(body))
        .map_err(|err| SerdeError::custom(format!("failed to build response: {err}")))
}

/// Finishes an error response: sets the status code and inserts the
/// [`ModeledErrorExtension`], preserving the legacy generated `IntoResponse`
/// behavior.
fn finish_error_response(
    mut response: http::Response<BoxBody>,
    status: u16,
    error_name: &str,
) -> http::Response<BoxBody> {
    *response.status_mut() = http::StatusCode::from_u16(status)
        .unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR);
    // `ModeledErrorExtension` requires `&'static str`; generated schemas are
    // `'static` but the `ModeledError::schema` seam erases that lifetime.
    // Interning is deduplicated and bounded by the number of distinct error
    // shape names in the process.
    response
        .extensions_mut()
        .insert(ModeledErrorExtension::new(aws_smithy_schema::intern_header_name(error_name)));
    response
}

macro_rules! log_serialize_failure {
    ($err:expr) => {
        tracing::error!(error = %$err, "failed to serialize response")
    };
}

// ============================================================================
// restJson1
// ============================================================================

impl ServerProtocol for RestJson1 {
    type Codec = JsonCodec;

    fn codec(&self) -> &Self::Codec {
        static CODEC: LazyLock<JsonCodec> = LazyLock::new(|| {
            JsonCodec::new(
                JsonCodecSettings::builder()
                    .use_json_name(true)
                    .default_timestamp_format(aws_smithy_types::date_time::Format::EpochSeconds)
                    .build(),
            )
        });
        &CODEC
    }

    fn serialize_output(
        &self,
        schema: &Schema<'_>,
        output: &dyn SerializableStruct,
        is_error: bool,
    ) -> Result<http::Response<BoxBody>, SerdeError> {
        // restJson1 carries no body discriminator; the error name travels in
        // the `x-amzn-errortype` header, stamped by `serialize_error`.
        let (body, headers) = serialize_body(self.codec(), schema, output, is_error)?;
        assemble_response(body, "application/json", headers)
    }

    fn serialize_error<E: HttpModeledError + ?Sized>(&self, error: &E) -> http::Response<BoxBody> {
        let schema = error.schema();
        let name = schema.shape_id().shape_name();
        match self.serialize_output(schema, &AsSerializable(error), true) {
            Ok(mut response) => {
                // Shape name only — the settled post-#1982 behavior.
                if let Ok(value) = http::HeaderValue::try_from(name) {
                    response
                        .headers_mut()
                        .insert(http::HeaderName::from_static("x-amzn-errortype"), value);
                }
                finish_error_response(response, error.status_code(), name)
            }
            Err(err) => {
                log_serialize_failure!(err);
                IntoResponse::<RestJson1>::into_response(
                    super::rest_json_1::runtime_error::RuntimeError::Serialization(
                        crate::Error::new(err),
                    ),
                )
            }
        }
    }
}

// ============================================================================
// awsJson 1.0 / 1.1
// ============================================================================

fn aws_json_codec() -> &'static JsonCodec {
    static CODEC: LazyLock<JsonCodec> = LazyLock::new(|| {
        JsonCodec::new(
            JsonCodecSettings::builder()
                .use_json_name(false)
                .default_timestamp_format(aws_smithy_types::date_time::Format::EpochSeconds)
                .build(),
        )
    });
    &CODEC
}

impl ServerProtocol for AwsJson1_0 {
    type Codec = JsonCodec;

    fn codec(&self) -> &Self::Codec {
        aws_json_codec()
    }

    fn serialize_output(
        &self,
        schema: &Schema<'_>,
        output: &dyn SerializableStruct,
        is_error: bool,
    ) -> Result<http::Response<BoxBody>, SerdeError> {
        let (body, _) = if is_error {
            // Full shape ID, written after the modeled members (legacy order).
            let wrapper = WithTypeLast {
                type_value: schema.shape_id().as_str(),
                inner: output,
            };
            serialize_body(self.codec(), schema, &wrapper, false)?
        } else {
            serialize_body(self.codec(), schema, output, false)?
        };
        assemble_response(body, "application/x-amz-json-1.0", Vec::new())
    }

    fn serialize_error<E: HttpModeledError + ?Sized>(&self, error: &E) -> http::Response<BoxBody> {
        let schema = error.schema();
        match self.serialize_output(schema, &AsSerializable(error), true) {
            Ok(response) => finish_error_response(
                response,
                error.status_code(),
                schema.shape_id().shape_name(),
            ),
            Err(err) => {
                log_serialize_failure!(err);
                IntoResponse::<AwsJson1_0>::into_response(
                    super::aws_json::runtime_error::RuntimeError::Serialization(crate::Error::new(
                        err,
                    )),
                )
            }
        }
    }
}

impl ServerProtocol for AwsJson1_1 {
    type Codec = JsonCodec;

    fn codec(&self) -> &Self::Codec {
        aws_json_codec()
    }

    fn serialize_output(
        &self,
        schema: &Schema<'_>,
        output: &dyn SerializableStruct,
        is_error: bool,
    ) -> Result<http::Response<BoxBody>, SerdeError> {
        let (body, _) = if is_error {
            // Shape name only, written after the modeled members (legacy order).
            let wrapper = WithTypeLast {
                type_value: schema.shape_id().shape_name(),
                inner: output,
            };
            serialize_body(self.codec(), schema, &wrapper, false)?
        } else {
            serialize_body(self.codec(), schema, output, false)?
        };
        assemble_response(body, "application/x-amz-json-1.1", Vec::new())
    }

    fn serialize_error<E: HttpModeledError + ?Sized>(&self, error: &E) -> http::Response<BoxBody> {
        let schema = error.schema();
        match self.serialize_output(schema, &AsSerializable(error), true) {
            Ok(response) => finish_error_response(
                response,
                error.status_code(),
                schema.shape_id().shape_name(),
            ),
            Err(err) => {
                log_serialize_failure!(err);
                IntoResponse::<AwsJson1_1>::into_response(
                    super::aws_json::runtime_error::RuntimeError::Serialization(crate::Error::new(
                        err,
                    )),
                )
            }
        }
    }
}

// ============================================================================
// rpcv2Cbor
// ============================================================================

impl ServerProtocol for RpcV2Cbor {
    type Codec = CborCodec;

    fn codec(&self) -> &Self::Codec {
        static CODEC: LazyLock<CborCodec> =
            LazyLock::new(|| CborCodec::new(CborCodecSettings::default()));
        &CODEC
    }

    fn serialize_output(
        &self,
        schema: &Schema<'_>,
        output: &dyn SerializableStruct,
        is_error: bool,
    ) -> Result<http::Response<BoxBody>, SerdeError> {
        let (body, _) = if is_error {
            // Full shape ID as the FIRST map entry (legacy
            // `AddTypeFieldToServerErrorsCborCustomization` order).
            let wrapper = WithTypeFirst {
                type_value: schema.shape_id().as_str(),
                inner: output,
            };
            serialize_body(self.codec(), schema, &wrapper, false)?
        } else {
            serialize_body(self.codec(), schema, output, false)?
        };
        let mut response = assemble_response(body, "application/cbor", Vec::new())?;
        response.headers_mut().insert(
            http::HeaderName::from_static("smithy-protocol"),
            http::HeaderValue::from_static("rpc-v2-cbor"),
        );
        Ok(response)
    }

    fn serialize_error<E: HttpModeledError + ?Sized>(&self, error: &E) -> http::Response<BoxBody> {
        let schema = error.schema();
        match self.serialize_output(schema, &AsSerializable(error), true) {
            Ok(response) => finish_error_response(
                response,
                error.status_code(),
                schema.shape_id().shape_name(),
            ),
            Err(err) => {
                log_serialize_failure!(err);
                IntoResponse::<RpcV2Cbor>::into_response(
                    super::rpc_v2_cbor::runtime_error::RuntimeError::Serialization(
                        crate::Error::new(err),
                    ),
                )
            }
        }
    }
}

// ============================================================================
// restXml
// ============================================================================

impl ServerProtocol for RestXml {
    type Codec = XmlCodec;

    fn codec(&self) -> &Self::Codec {
        static CODEC: LazyLock<XmlCodec> =
            LazyLock::new(|| XmlCodec::new(XmlCodecSettings::default()));
        &CODEC
    }

    fn serialize_output(
        &self,
        schema: &Schema<'_>,
        output: &dyn SerializableStruct,
        is_error: bool,
    ) -> Result<http::Response<BoxBody>, SerdeError> {
        // Known divergence, deliberate: today's generated restXml server error
        // bodies are broken (bare `<Error>` envelope no client parses, and the
        // runtime discards pre-rendered validation/framework bodies in favor of
        // a literal `"{}"`). Freezing that behavior would freeze a bug, so the
        // schema path serializes the error structure through the XML codec
        // as-is. See assumptions register B4/B6.
        let (body, headers) = serialize_body(self.codec(), schema, output, is_error)?;
        assemble_response(body, "application/xml", headers)
    }

    fn serialize_error<E: HttpModeledError + ?Sized>(&self, error: &E) -> http::Response<BoxBody> {
        let schema = error.schema();
        let name = schema.shape_id().shape_name();
        match self.serialize_output(schema, &AsSerializable(error), true) {
            Ok(response) => finish_error_response(response, error.status_code(), name),
            Err(err) => {
                log_serialize_failure!(err);
                IntoResponse::<RestXml>::into_response(
                    super::rest_xml::runtime_error::RuntimeError::Serialization(crate::Error::new(
                        err,
                    )),
                )
            }
        }
    }
}
