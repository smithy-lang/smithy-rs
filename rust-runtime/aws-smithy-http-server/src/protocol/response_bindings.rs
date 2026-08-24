/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Response-binding interpretation for the REST protocols (2b).
//!
//! Serializing an output (or error) structure on a REST protocol splits its
//! top-level members by binding, read off each member schema:
//!
//! - `@httpHeader` — diverted to response headers (empty strings skipped,
//!   mirroring the legacy generated `ser_*_headers` functions).
//! - `@httpPrefixHeaders` — map entries become `prefix + key` headers.
//! - `@httpResponseCode` — captured as the response status; never written to
//!   the body.
//! - `@httpPayload` — the body IS that member: blob/string raw (content type
//!   from `@mediaType`, else `application/octet-stream` / `text/plain`),
//!   structure/union/document through the codec.
//! - everything else — forwarded to the codec body serializer.
//!
//! Non-REST protocols never enter this module: they serialize body-only.

use std::cell::{Cell, RefCell};

use aws_smithy_schema::codec::{Codec, FinishSerializer};
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::{Schema, ShapeType};
use aws_smithy_types::{BigDecimal, BigInteger, DateTime, Document};

type CapturedHeaders = RefCell<Vec<(http::HeaderName, http::HeaderValue)>>;

/// How the response body was produced, which determines the `Content-Type`.
#[derive(Debug)]
pub(crate) enum BodyKind {
    /// Codec-framed document body: the protocol's content type applies.
    /// (Structure/union/document `@httpPayload` bodies are codec-framed too.)
    Codec,
    /// Raw `@httpPayload` bytes with a payload-derived content type.
    Raw { content_type: String },
    /// An `@httpPayload` member was modeled but unset: empty body, no
    /// content type.
    Empty,
}

/// The pieces of a serialized REST response body, before assembly.
#[derive(Debug)]
pub(crate) struct SplitResponse {
    pub(crate) body: Vec<u8>,
    pub(crate) kind: BodyKind,
    pub(crate) headers: Vec<(http::HeaderName, http::HeaderValue)>,
    /// Captured `@httpResponseCode` member value, if bound and set.
    pub(crate) status: Option<u16>,
}

/// Returns `true` if any top-level member of `schema` carries a response
/// binding this module interprets.
fn has_response_bound_members(schema: &Schema<'_>) -> bool {
    schema.members().iter().any(|m| {
        m.http_header().is_some()
            || m.http_prefix_headers().is_some()
            || m.http_response_code().is_some()
            || m.http_payload().is_some()
    })
}

/// Serializes `value` against `schema` through `codec`, interpreting response
/// bindings off the member schemas (REST protocols).
///
/// `split_bindings = false` short-circuits to a plain codec body (RPC
/// protocols and the common no-bindings case share this path).
pub(crate) fn serialize_split<C: Codec>(
    codec: &C,
    schema: &Schema<'_>,
    value: &dyn SerializableStruct,
    split_bindings: bool,
) -> Result<SplitResponse, SerdeError> {
    if !split_bindings || !has_response_bound_members(schema) {
        let mut serializer = codec.create_serializer();
        serializer.write_struct(schema, value)?;
        return Ok(SplitResponse {
            body: serializer.finish(),
            kind: BodyKind::Codec,
            headers: Vec::new(),
            status: None,
        });
    }

    let has_payload_member = schema.members().iter().any(|m| m.http_payload().is_some());
    let headers = CapturedHeaders::default();
    let status = Cell::new(None);
    let payload = RefCell::new(None);

    let body = if has_payload_member {
        // `@httpPayload` forbids other body members: drive the members
        // directly through the splitter (no codec framing) and take the
        // captured payload as the body.
        let mut sink = NoBodySerializer;
        let mut splitter = ResponseBindingSplitter {
            body: &mut sink,
            codec,
            headers: &headers,
            status: &status,
            payload: &payload,
            payload_mode: true,
        };
        value.serialize_members(&mut splitter)?;
        Vec::new()
    } else {
        let mut body_serializer = codec.create_serializer();
        {
            let wrapper = SplitBindings {
                inner: value,
                codec,
                headers: &headers,
                status: &status,
                payload: &payload,
            };
            body_serializer.write_struct(schema, &wrapper)?;
        }
        body_serializer.finish()
    };

    let (body, kind) = if has_payload_member {
        match payload.into_inner() {
            // A payload-derived content type means raw bytes; `None` means
            // the payload was codec-serialized (structure/union/document).
            Some(CapturedPayload {
                bytes,
                content_type: Some(content_type),
            }) => (bytes, BodyKind::Raw { content_type }),
            Some(CapturedPayload {
                bytes,
                content_type: None,
            }) => (bytes, BodyKind::Codec),
            None => (Vec::new(), BodyKind::Empty),
        }
    } else {
        (body, BodyKind::Codec)
    };

    Ok(SplitResponse {
        body,
        kind,
        headers: headers.into_inner(),
        status: status.get(),
    })
}

/// A captured `@httpPayload` member value.
struct CapturedPayload {
    bytes: Vec<u8>,
    /// `Some` = payload-derived content type; `None` = the protocol's codec
    /// content type applies (structure/union/document payloads).
    content_type: Option<String>,
}

/// Wrapper diverting bound top-level members into their sinks while
/// forwarding everything else to the codec body serializer.
struct SplitBindings<'a, C> {
    inner: &'a dyn SerializableStruct,
    codec: &'a C,
    headers: &'a CapturedHeaders,
    status: &'a Cell<Option<u16>>,
    payload: &'a RefCell<Option<CapturedPayload>>,
}

impl<C: Codec> SerializableStruct for SplitBindings<'_, C> {
    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        let mut splitter = ResponseBindingSplitter {
            body: serializer,
            codec: self.codec,
            headers: self.headers,
            status: self.status,
            payload: self.payload,
            payload_mode: false,
        };
        self.inner.serialize_members(&mut splitter)
    }
}

/// Body sink for the `@httpPayload` path: no non-bound body members can
/// exist alongside a payload member, so any write reaching this is a bug in
/// the model/schema.
struct NoBodySerializer;

macro_rules! no_body_writes {
    ($($method:ident($($arg:ty),*)),+ $(,)?) => {
        $(
            fn $method(&mut self, _: &Schema<'_>, $(_: $arg),*) -> Result<(), SerdeError> {
                Err(SerdeError::custom(
                    "a member without a response binding cannot coexist with an @httpPayload member",
                ))
            }
        )+
    };
}

impl ShapeSerializer for NoBodySerializer {
    no_body_writes! {
        write_struct(&dyn SerializableStruct),
        write_list(&dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>),
        write_map(&dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>),
        write_boolean(bool),
        write_byte(i8),
        write_short(i16),
        write_integer(i32),
        write_long(i64),
        write_float(f32),
        write_double(f64),
        write_big_integer(&BigInteger),
        write_big_decimal(&BigDecimal),
        write_string(&str),
        write_blob(&[u8]),
        write_timestamp(&DateTime),
        write_document(&Document),
        write_null(),
    }
}

pub(crate) fn capture_header(
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
pub(crate) fn format_header_timestamp(
    schema: &Schema<'_>,
    value: &DateTime,
) -> Result<String, SerdeError> {
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

/// Serializer that intercepts bound member writes and forwards the rest to
/// the wrapped body serializer.
struct ResponseBindingSplitter<'a, C> {
    body: &'a mut dyn ShapeSerializer,
    codec: &'a C,
    headers: &'a CapturedHeaders,
    status: &'a Cell<Option<u16>>,
    payload: &'a RefCell<Option<CapturedPayload>>,
    /// True when the shape has an `@httpPayload` member. In that mode any
    /// structure/union/document write reaching the splitter IS the payload:
    /// generated walkers pass the payload member's TARGET schema (which
    /// carries the framing but not the member's binding traits) — the client
    /// codegen convention.
    payload_mode: bool,
}

impl<C: Codec> ResponseBindingSplitter<'_, C> {
    fn is_header(&self, schema: &Schema<'_>) -> bool {
        schema.http_header().is_some()
    }

    fn capture_status(&self, value: i64) -> Result<(), SerdeError> {
        let status = u16::try_from(value)
            .ok()
            .filter(|code| (100..1000).contains(code))
            .ok_or_else(|| {
                SerdeError::custom(format!(
                    "invalid bound HTTP status code; status codes must be inside the 100-999 range: {value}"
                ))
            })?;
        self.status.set(Some(status));
        Ok(())
    }

    fn capture_payload(&self, bytes: Vec<u8>, content_type: Option<String>) {
        *self.payload.borrow_mut() = Some(CapturedPayload { bytes, content_type });
    }
}

macro_rules! split_int {
    ($fn_name:ident, $ty:ty) => {
        fn $fn_name(&mut self, schema: &Schema<'_>, value: $ty) -> Result<(), SerdeError> {
            if schema.http_response_code().is_some() {
                self.capture_status(value as i64)
            } else if self.is_header(schema) {
                let mut encoder = aws_smithy_types::primitive::Encoder::from(value);
                capture_header(self.headers, schema, encoder.encode())
            } else {
                self.body.$fn_name(schema, value)
            }
        }
    };
}

macro_rules! split_scalar {
    ($fn_name:ident, $ty:ty) => {
        fn $fn_name(&mut self, schema: &Schema<'_>, value: $ty) -> Result<(), SerdeError> {
            if self.is_header(schema) {
                let mut encoder = aws_smithy_types::primitive::Encoder::from(value);
                capture_header(self.headers, schema, encoder.encode())
            } else {
                self.body.$fn_name(schema, value)
            }
        }
    };
}

impl<C: Codec> ShapeSerializer for ResponseBindingSplitter<'_, C> {
    fn write_struct(
        &mut self,
        schema: &Schema<'_>,
        value: &dyn SerializableStruct,
    ) -> Result<(), SerdeError> {
        if self.payload_mode || schema.http_payload().is_some() {
            // Structure/union payload: the payload member's own framing IS
            // the body — serialize it standalone through the codec.
            let mut serializer = self.codec.create_serializer();
            serializer.write_struct(schema, value)?;
            self.capture_payload(serializer.finish(), None);
            return Ok(());
        }
        // `@httpHeader` / `@httpResponseCode` cannot target structures.
        self.body.write_struct(schema, value)
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
                sink: self.headers,
                outer: schema,
            };
            write_elements(&mut collector)
        } else {
            self.body.write_list(schema, write_elements)
        }
    }

    fn write_map(
        &mut self,
        schema: &Schema<'_>,
        write_entries: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        if let Some(prefix) = schema.http_prefix_headers() {
            let mut collector = PrefixHeaderCollector {
                prefix: prefix.value(),
                sink: self.headers,
                pending_key: None,
            };
            write_entries(&mut collector)
        } else {
            self.body.write_map(schema, write_entries)
        }
    }

    split_scalar!(write_boolean, bool);
    split_int!(write_byte, i8);
    split_int!(write_short, i16);
    split_int!(write_integer, i32);
    split_int!(write_long, i64);
    split_scalar!(write_float, f32);
    split_scalar!(write_double, f64);

    fn write_big_integer(
        &mut self,
        schema: &Schema<'_>,
        value: &BigInteger,
    ) -> Result<(), SerdeError> {
        if self.is_header(schema) {
            capture_header(self.headers, schema, value.as_ref())
        } else {
            self.body.write_big_integer(schema, value)
        }
    }

    fn write_big_decimal(
        &mut self,
        schema: &Schema<'_>,
        value: &BigDecimal,
    ) -> Result<(), SerdeError> {
        if self.is_header(schema) {
            capture_header(self.headers, schema, value.as_ref())
        } else {
            self.body.write_big_decimal(schema, value)
        }
    }

    fn write_string(&mut self, schema: &Schema<'_>, value: &str) -> Result<(), SerdeError> {
        if schema.http_payload().is_some() {
            let content_type = schema
                .media_type()
                .map(|m| m.value().to_string())
                .unwrap_or_else(|| "text/plain".to_string());
            self.capture_payload(value.as_bytes().to_vec(), Some(content_type));
            return Ok(());
        }
        if self.is_header(schema) {
            // `@mediaType` on a header-bound string: base64-encode.
            if schema.media_type().is_some() {
                let encoded = aws_smithy_types::base64::encode(value.as_bytes());
                return capture_header(self.headers, schema, &encoded);
            }
            return capture_header(self.headers, schema, value);
        }
        self.body.write_string(schema, value)
    }

    fn write_blob(&mut self, schema: &Schema<'_>, value: &[u8]) -> Result<(), SerdeError> {
        if schema.http_payload().is_some() {
            let content_type = schema
                .media_type()
                .map(|m| m.value().to_string())
                .unwrap_or_else(|| "application/octet-stream".to_string());
            self.capture_payload(value.to_vec(), Some(content_type));
            return Ok(());
        }
        if self.is_header(schema) {
            return capture_header(self.headers, schema, &aws_smithy_types::base64::encode(value));
        }
        self.body.write_blob(schema, value)
    }

    fn write_timestamp(&mut self, schema: &Schema<'_>, value: &DateTime) -> Result<(), SerdeError> {
        if self.is_header(schema) {
            let formatted = format_header_timestamp(schema, value)?;
            capture_header(self.headers, schema, &formatted)
        } else {
            self.body.write_timestamp(schema, value)
        }
    }

    fn write_document(&mut self, schema: &Schema<'_>, value: &Document) -> Result<(), SerdeError> {
        if self.payload_mode || schema.http_payload().is_some() {
            // The document VALUE is the body: serialize against the prelude
            // document schema, not the member schema — a member schema would
            // make the codec emit a `"memberName":` key fragment.
            let mut serializer = self.codec.create_serializer();
            serializer.write_document(&aws_smithy_schema::prelude::DOCUMENT, value)?;
            self.capture_payload(serializer.finish(), None);
            return Ok(());
        }
        self.body.write_document(schema, value)
    }

    fn write_null(&mut self, schema: &Schema<'_>) -> Result<(), SerdeError> {
        if self.is_header(schema)
            || schema.http_response_code().is_some()
            || schema.http_payload().is_some()
            || schema.http_prefix_headers().is_some()
        {
            // A null bound member is simply not sent.
            Ok(())
        } else {
            self.body.write_null(schema)
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
        // Elements of a header-bound list are quoted when they contain `,` or
        // `"` (RFC 9110 list syntax) — mirroring the legacy generated
        // serializers' `quote_header_value` usage.
        let quoted = aws_smithy_http::header::quote_header_value(value);
        capture_header(self.sink, self.outer, quoted.as_ref())
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

/// Collects `@httpPrefixHeaders` map entries: each `key → value` entry becomes
/// a `prefix + key` header. Map keys and values are strings by the Smithy
/// binding rules.
struct PrefixHeaderCollector<'a> {
    prefix: &'a str,
    sink: &'a CapturedHeaders,
    pending_key: Option<String>,
}

macro_rules! prefix_reject {
    ($($method:ident($($arg:ty),*)),+ $(,)?) => {
        $(
            fn $method(&mut self, _: &Schema<'_>, $(_: $arg),*) -> Result<(), SerdeError> {
                Err(SerdeError::custom(
                    "@httpPrefixHeaders maps have string keys and string values",
                ))
            }
        )+
    };
}

impl ShapeSerializer for PrefixHeaderCollector<'_> {
    fn write_string(&mut self, _schema: &Schema<'_>, value: &str) -> Result<(), SerdeError> {
        match self.pending_key.take() {
            None => {
                self.pending_key = Some(value.to_string());
                Ok(())
            }
            Some(key) => {
                // Mirror the header skip-empty rule.
                if value.is_empty() {
                    return Ok(());
                }
                let name = http::HeaderName::try_from(format!("{}{}", self.prefix, key))
                    .map_err(|err| {
                        SerdeError::custom(format!(
                            "`{}{}` cannot be used as a header name: {err}",
                            self.prefix, key
                        ))
                    })?;
                let header_value = http::HeaderValue::try_from(value).map_err(|err| {
                    SerdeError::custom(format!(
                        "`{value}` cannot be used as a header value: {err}"
                    ))
                })?;
                self.sink.borrow_mut().push((name, header_value));
                Ok(())
            }
        }
    }

    fn write_null(&mut self, _schema: &Schema<'_>) -> Result<(), SerdeError> {
        // A null map value: drop the pending key, send nothing.
        self.pending_key = None;
        Ok(())
    }

    prefix_reject! {
        write_struct(&dyn SerializableStruct),
        write_list(&dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>),
        write_map(&dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>),
        write_boolean(bool),
        write_byte(i8),
        write_short(i16),
        write_integer(i32),
        write_long(i64),
        write_float(f32),
        write_double(f64),
        write_big_integer(&BigInteger),
        write_big_decimal(&BigDecimal),
        write_blob(&[u8]),
        write_timestamp(&DateTime),
        write_document(&Document),
    }
}

/// Resolves the response status per the 2a rule: captured `@httpResponseCode`
/// member if bound and set, else the `@http` trait's `code`, else `200`.
pub(crate) fn resolve_status(captured: Option<u16>, schema: &Schema<'_>) -> u16 {
    captured
        .or_else(|| schema.http().map(|h| h.code()))
        .unwrap_or(200)
}

/// `true` if `schema` describes a shape whose `@httpPayload`-bound member
/// makes the body raw rather than codec-framed. Used by callers that need to
/// know the content type before serializing.
#[allow(dead_code)]
pub(crate) fn payload_member<'s>(schema: &'s Schema<'s>) -> Option<&'s Schema<'s>> {
    schema
        .members()
        .iter()
        .find(|m| m.http_payload().is_some())
        .map(|m| &**m)
}

/// Retained helper: `true` if the shape has any `ShapeType::Structure` /
/// `Union` payload member (body framing comes from the payload).
#[allow(dead_code)]
pub(crate) fn has_struct_payload(schema: &Schema<'_>) -> bool {
    schema.members().iter().any(|m| {
        m.http_payload().is_some()
            && matches!(m.shape_type(), ShapeType::Structure | ShapeType::Union)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
    use aws_smithy_schema::traits::HttpTrait;
    use aws_smithy_schema::ShapeId;

    fn json_codec() -> JsonCodec {
        JsonCodec::new(
            JsonCodecSettings::builder()
                .use_json_name(true)
                .default_timestamp_format(aws_smithy_types::date_time::Format::EpochSeconds)
                .build(),
        )
    }

    static CODE_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#Out$code", "test", "Out"),
        ShapeType::Integer,
        "code",
        0,
    )
    .with_http_response_code();
    static HDR_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#Out$hdr", "test", "Out"),
        ShapeType::String,
        "hdr",
        1,
    )
    .with_http_header("x-hdr");
    static META_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#Out$meta", "test", "Out"),
        ShapeType::Map,
        "meta",
        2,
    )
    .with_http_prefix_headers("x-meta-");
    static BODY_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#Out$msg", "test", "Out"),
        ShapeType::String,
        "msg",
        3,
    );
    static OUT_MEMBERS: [&Schema<'static>; 4] =
        [&CODE_MEMBER, &HDR_MEMBER, &META_MEMBER, &BODY_MEMBER];
    static OUT_SCHEMA: Schema<'static> = Schema::new_struct(
        ShapeId::from_parts("test#Out", "test", "Out"),
        ShapeType::Structure,
        &OUT_MEMBERS,
    )
    .with_http(HttpTrait::new("POST", "/op", Some(201)));

    struct Out;
    impl SerializableStruct for Out {
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            s.write_integer(&CODE_MEMBER, 202)?;
            s.write_string(&HDR_MEMBER, "hval")?;
            s.write_map(&META_MEMBER, &{
                |m: &mut dyn ShapeSerializer| {
                    m.write_string(&META_MEMBER, "color")?;
                    m.write_string(&META_MEMBER, "red")
                }
            })?;
            s.write_string(&BODY_MEMBER, "hello")
        }
    }

    #[test]
    fn split_bindings_and_status() {
        // REST path: @httpHeader and @httpPrefixHeaders divert to headers,
        // @httpResponseCode is captured (never in the body), the rest is the
        // codec body.
        let codec = json_codec();
        let split = serialize_split(&codec, &OUT_SCHEMA, &Out, true).unwrap();
        assert_eq!(split.status, Some(202));
        assert!(matches!(split.kind, BodyKind::Codec));
        assert_eq!(
            String::from_utf8(split.body.clone()).unwrap(),
            r#"{"msg":"hello"}"#
        );
        let headers: Vec<(String, String)> = split
            .headers
            .iter()
            .map(|(n, v)| (n.to_string(), v.to_str().unwrap().to_string()))
            .collect();
        assert!(headers.contains(&("x-hdr".to_string(), "hval".to_string())));
        assert!(headers.contains(&("x-meta-color".to_string(), "red".to_string())));

        // Status resolution: captured @httpResponseCode, else @http code,
        // else 200.
        assert_eq!(resolve_status(split.status, &OUT_SCHEMA), 202);
        assert_eq!(resolve_status(None, &OUT_SCHEMA), 201);
        static PLAIN: Schema<'static> = Schema::new(
            ShapeId::from_parts("test#Plain", "test", "Plain"),
            ShapeType::Structure,
        );
        assert_eq!(resolve_status(None, &PLAIN), 200);

        // RPC path (split_bindings = false): everything, bound or not, goes
        // to the body.
        let split = serialize_split(&codec, &OUT_SCHEMA, &Out, false).unwrap();
        assert_eq!(split.status, None);
        assert!(split.headers.is_empty());
        let body = String::from_utf8(split.body).unwrap();
        assert!(body.contains("\"code\":202"));
        assert!(body.contains("\"hdr\":\"hval\""));
        assert!(body.contains("\"msg\":\"hello\""));
    }

    // ------------------------------------------------------------------
    // @httpPayload
    // ------------------------------------------------------------------

    static BLOB_PAYLOAD_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#POut$data", "test", "POut"),
        ShapeType::Blob,
        "data",
        0,
    )
    .with_http_payload()
    .with_media_type("image/png");
    static BLOB_OUT_MEMBERS: [&Schema<'static>; 1] = [&BLOB_PAYLOAD_MEMBER];
    static BLOB_OUT_SCHEMA: Schema<'static> = Schema::new_struct(
        ShapeId::from_parts("test#POut", "test", "POut"),
        ShapeType::Structure,
        &BLOB_OUT_MEMBERS,
    );

    struct BlobOut(Option<Vec<u8>>);
    impl SerializableStruct for BlobOut {
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            if let Some(bytes) = &self.0 {
                s.write_blob(&BLOB_PAYLOAD_MEMBER, bytes)?;
            }
            Ok(())
        }
    }

    #[test]
    fn payload_bodies() {
        // Blob payload: raw bytes, content type from @mediaType.
        let codec = json_codec();
        let split =
            serialize_split(&codec, &BLOB_OUT_SCHEMA, &BlobOut(Some(vec![1, 2, 3])), true)
                .unwrap();
        assert_eq!(split.body, vec![1, 2, 3]);
        match split.kind {
            BodyKind::Raw { content_type } => assert_eq!(content_type, "image/png"),
            other => panic!("expected raw body, got {other:?}"),
        }

        // Unset payload member: empty body, no content type.
        let split = serialize_split(&codec, &BLOB_OUT_SCHEMA, &BlobOut(None), true).unwrap();
        assert!(split.body.is_empty());
        assert!(matches!(split.kind, BodyKind::Empty));

        // Structure payload (written against its TARGET schema, the codegen
        // convention): the body is the codec document of that member alone.
        let split = serialize_split(&codec, &STRUCT_OUT_SCHEMA, &StructOut, true).unwrap();
        assert!(matches!(split.kind, BodyKind::Codec));
        assert_eq!(String::from_utf8(split.body).unwrap(), r#"{"f":"v"}"#);
    }

    static STRUCT_PAYLOAD_TARGET: Schema<'static> = Schema::new(
        ShapeId::from_parts("test#Nested", "test", "Nested"),
        ShapeType::Structure,
    );
    static STRUCT_PAYLOAD_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#SOut$nested", "test", "SOut"),
        ShapeType::Structure,
        "nested",
        0,
    )
    .with_http_payload();
    static STRUCT_OUT_MEMBERS: [&Schema<'static>; 1] = [&STRUCT_PAYLOAD_MEMBER];
    static STRUCT_OUT_SCHEMA: Schema<'static> = Schema::new_struct(
        ShapeId::from_parts("test#SOut", "test", "SOut"),
        ShapeType::Structure,
        &STRUCT_OUT_MEMBERS,
    );

    struct StructOut;
    impl SerializableStruct for StructOut {
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            struct Nested;
            impl SerializableStruct for Nested {
                fn serialize_members(
                    &self,
                    s: &mut dyn ShapeSerializer,
                ) -> Result<(), SerdeError> {
                    static F: Schema<'static> = Schema::new_member(
                        ShapeId::from_parts("test#Nested$f", "test", "Nested"),
                        ShapeType::String,
                        "f",
                        0,
                    );
                    s.write_string(&F, "v")
                }
            }
            // Codegen convention: the payload struct is written against its
            // TARGET schema, so the body framing comes from the target.
            s.write_struct(&STRUCT_PAYLOAD_TARGET, &Nested)
        }
    }

}
