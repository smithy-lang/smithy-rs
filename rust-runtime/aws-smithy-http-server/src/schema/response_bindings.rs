/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Response-binding interpretation for the REST protocols.
//!
//! Serializing an output (or error) structure on a REST protocol splits its
//! top-level members by binding, read off each member schema:
//!
//! - `@httpHeader` — diverted to response headers (empty strings skipped,
//!   mirroring the generated `ser_*_headers` functions).
//! - `@httpPrefixHeaders` — map entries become `prefix + key` headers.
//! - `@httpResponseCode` — captured as the response status; never written to
//!   the body.
//! - `@httpPayload` — the body IS that member: blob/string raw,
//!   structure/union/document through the codec.
//! - everything else — forwarded to the codec body serializer.
//!
//! The response `Content-Type` is not decided here: it is the protocol's policy, derived from
//! the schema alone. Non-REST protocols serialize body-only through the same entry point.

use std::cell::{Cell, RefCell};

use aws_smithy_schema::codec::{Codec, FinishSerializer};
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::Schema;
use aws_smithy_types::{BigDecimal, BigInteger, DateTime, Document};

type CapturedHeaders = RefCell<Vec<(http::HeaderName, http::HeaderValue)>>;

/// The pieces of a serialized response body, before assembly.
#[derive(Debug)]
pub(crate) struct ResponseParts {
    /// `Bytes` so a blob payload moves into the response without being copied.
    pub(crate) body: bytes::Bytes,
    pub(crate) headers: Vec<(http::HeaderName, http::HeaderValue)>,
    /// Captured `@httpResponseCode` member value, if bound and set.
    pub(crate) status: Option<u16>,
}

/// The kind of value being serialized into a response.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ResponseValueKind {
    /// Successful operation output. With no body members the HTTP body is empty, unless
    /// `empty_document` is set and the schema was modeled by the user (it then carries an
    /// original name): the JSON and CBOR protocols write an empty document, `{}` or `bf ff`, for
    /// such outputs, exactly as the legacy generated serializers do. The same flag makes an unset
    /// structure `@httpPayload` an empty document rather than an empty body.
    OperationOutput { empty_document: bool },
    /// The head of a streaming output: bound members only, nothing is written to the body.
    StreamingOutput,
    /// Modeled error. Even an empty error structure is serialized through the
    /// protocol codec, for example `{}` on restJson1.
    ModeledError,
}

/// Whether `@http*` response bindings are interpreted or everything is codec body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseBindings {
    /// `@httpHeader`, `@httpPrefixHeaders`, `@httpResponseCode` and `@httpPayload` divert
    /// members out of the body (the REST protocols).
    Rest,
    /// Every member is serialized into the codec body (the RPC protocols).
    BodyOnly,
}

/// Returns `true` if any top-level member of `schema` carries a response
/// binding this module interprets.
pub(crate) fn has_response_bound_members(schema: &Schema<'_>) -> bool {
    schema.members().iter().any(|m| {
        m.http_header().is_some()
            || m.http_prefix_headers().is_some()
            || m.http_response_code().is_some()
            || m.http_payload().is_some()
    })
}

/// Serializes `value` against `schema` through `codec` into HTTP response
/// parts, deriving the plan from the schema on the spot.
pub(crate) fn serialize_response_parts<C: Codec>(
    codec: &C,
    schema: &Schema<'_>,
    value: &dyn SerializableStruct,
    bindings: ResponseBindings,
    value_kind: ResponseValueKind,
) -> Result<ResponseParts, SerdeError> {
    let plan = CompiledResponsePlan::compile(schema, bindings, value_kind);
    serialize_response_parts_compiled(codec, schema, value, &plan)
}

/// The response serialization strategy, derived from the schema for each response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResponseStrategy {
    Empty,
    BindingsOnly,
    CodecBody,
    SplitBody,
    Payload,
}

#[derive(Debug, Clone)]
enum ResponseMemberPlan {
    Body,
    Header {
        name: http::HeaderName,
        media_type: bool,
        timestamp_format: HeaderTimestampFormat,
        sensitive: bool,
    },
    PrefixHeaders {
        prefix: String,
        key_sensitive: bool,
        value_sensitive: bool,
    },
    Status,
    Payload {
        /// Blob and string payloads are captured as raw bytes; structure, union and document
        /// payloads are codec documents.
        raw: bool,
    },
}

#[derive(Debug, Clone, Copy)]
enum HeaderTimestampFormat {
    EpochSeconds,
    DateTime,
    HttpDate,
}

#[derive(Debug, Clone)]
pub(crate) struct CompiledResponsePlan {
    strategy: ResponseStrategy,
    members: Box<[ResponseMemberPlan]>,
    /// An unset structure `@httpPayload` is written as the codec's empty document.
    unset_structure_payload_is_document: bool,
}

impl CompiledResponsePlan {
    pub(crate) fn compile(schema: &Schema<'_>, bindings: ResponseBindings, value_kind: ResponseValueKind) -> Self {
        let has_bindings = bindings == ResponseBindings::Rest && has_response_bound_members(schema);
        let writes_body = match value_kind {
            ResponseValueKind::ModeledError => true,
            ResponseValueKind::StreamingOutput => false,
            ResponseValueKind::OperationOutput { empty_document } => {
                has_output_body_members(schema, bindings) || (empty_document && schema.original_name().is_some())
            }
        };
        let unset_structure_payload_is_document =
            matches!(value_kind, ResponseValueKind::OperationOutput { empty_document: true });
        let strategy = if !writes_body {
            if has_bindings {
                ResponseStrategy::BindingsOnly
            } else {
                ResponseStrategy::Empty
            }
        } else if !has_bindings {
            ResponseStrategy::CodecBody
        } else if schema.members().iter().any(|member| member.http_payload().is_some()) {
            ResponseStrategy::Payload
        } else {
            ResponseStrategy::SplitBody
        };
        let members = if matches!(
            strategy,
            ResponseStrategy::BindingsOnly | ResponseStrategy::SplitBody | ResponseStrategy::Payload
        ) {
            let member_count = schema
                .members()
                .iter()
                .filter_map(|member| member.member_index())
                .max()
                .map_or(0, |index| index + 1);
            let mut members = vec![ResponseMemberPlan::Body; member_count];
            for member in schema.members() {
                if let Some(index) = member.member_index() {
                    members[index] = compile_member_plan(member, bindings, schema.sensitive().is_some());
                }
            }
            members.into_boxed_slice()
        } else {
            Box::new([])
        };
        Self {
            strategy,
            members,
            unset_structure_payload_is_document,
        }
    }

    fn member(&self, schema: &Schema<'_>) -> &ResponseMemberPlan {
        schema
            .member_index()
            .and_then(|index| self.members.get(index))
            .unwrap_or(&ResponseMemberPlan::Body)
    }
}

fn compile_member_plan(
    schema: &Schema<'_>,
    bindings: ResponseBindings,
    container_sensitive: bool,
) -> ResponseMemberPlan {
    if bindings == ResponseBindings::BodyOnly {
        return ResponseMemberPlan::Body;
    }
    if schema.http_response_code().is_some() {
        return ResponseMemberPlan::Status;
    }
    if let Some(header) = schema.http_header() {
        let name = http::HeaderName::try_from(header.value()).unwrap_or_else(|err| {
            panic!(
                "invalid @httpHeader name `{}` in compiled response schema: {err}",
                header.value()
            )
        });
        let timestamp_format = match schema.timestamp_format().map(|value| value.format()) {
            Some(aws_smithy_schema::traits::TimestampFormat::EpochSeconds) => HeaderTimestampFormat::EpochSeconds,
            Some(aws_smithy_schema::traits::TimestampFormat::DateTime) => HeaderTimestampFormat::DateTime,
            Some(_) | None => HeaderTimestampFormat::HttpDate,
        };
        return ResponseMemberPlan::Header {
            name,
            media_type: schema.media_type().is_some()
                || schema.member().is_some_and(|element| element.media_type().is_some()),
            timestamp_format,
            sensitive: container_sensitive
                || schema.sensitive().is_some()
                || schema.member().is_some_and(|element| element.sensitive().is_some()),
        };
    }
    if let Some(prefix) = schema.http_prefix_headers() {
        return ResponseMemberPlan::PrefixHeaders {
            prefix: prefix.value().to_string(),
            key_sensitive: container_sensitive
                || schema.sensitive().is_some()
                || schema.key().is_some_and(|key| key.sensitive().is_some()),
            value_sensitive: container_sensitive
                || schema.sensitive().is_some()
                || schema.member().is_some_and(|value| value.sensitive().is_some()),
        };
    }
    if schema.http_payload().is_some() {
        let raw = matches!(
            schema.shape_type(),
            aws_smithy_schema::ShapeType::String | aws_smithy_schema::ShapeType::Blob
        );
        return ResponseMemberPlan::Payload { raw };
    }
    ResponseMemberPlan::Body
}

pub(crate) fn serialize_response_parts_compiled<C: Codec>(
    codec: &C,
    schema: &Schema<'_>,
    value: &dyn SerializableStruct,
    plan: &CompiledResponsePlan,
) -> Result<ResponseParts, SerdeError> {
    if matches!(plan.strategy, ResponseStrategy::Empty | ResponseStrategy::BindingsOnly) {
        let headers = CapturedHeaders::default();
        // `SerializableStruct::serialize_members` receives `&self`. The splitter is reached through
        // that shared reference, so captured response bindings require interior mutability.
        let status = Cell::new(None);
        if matches!(plan.strategy, ResponseStrategy::BindingsOnly) {
            let payload = RefCell::new(None);
            let mut sink = NoBodySerializer { discard: true };
            let mut splitter = ResponseBindingSplitter {
                body: &mut sink,
                codec,
                headers: &headers,
                status: &status,
                payload: &payload,
                payload_mode: false,
                capture_bindings: true,
                plan,
            };
            value.serialize_members(&mut splitter)?;
        }
        return Ok(ResponseParts {
            body: bytes::Bytes::new(),
            headers: headers.into_inner(),
            status: status.get(),
        });
    }

    if matches!(plan.strategy, ResponseStrategy::CodecBody) {
        let mut serializer = codec.create_serializer();
        serializer.write_struct(schema, value)?;
        return Ok(ResponseParts {
            body: serializer.finish().into(),
            headers: Vec::new(),
            status: None,
        });
    }

    let has_payload_member = matches!(plan.strategy, ResponseStrategy::Payload);
    let headers = CapturedHeaders::default();
    let status = Cell::new(None);
    let payload = RefCell::new(None);

    let body = if has_payload_member {
        // `@httpPayload` forbids other body members: drive the members
        // directly through the splitter (no codec framing) and take the
        // captured payload as the body.
        let mut sink = NoBodySerializer { discard: false };
        let mut splitter = ResponseBindingSplitter {
            body: &mut sink,
            codec,
            headers: &headers,
            status: &status,
            payload: &payload,
            payload_mode: true,
            capture_bindings: true,
            plan,
        };
        value.serialize_members(&mut splitter)?;
        bytes::Bytes::new()
    } else {
        let mut body_serializer = codec.create_serializer();
        {
            let wrapper = SplitBindings {
                inner: value,
                codec,
                headers: &headers,
                status: &status,
                payload: &payload,
                plan,
                capture_bindings: Cell::new(true),
            };
            body_serializer.write_struct(schema, &wrapper)?;
        }
        body_serializer.finish().into()
    };

    // An unset payload member is an empty body, except an unset structure payload on the protocols
    // whose legacy serializers write the codec's empty document for it (`{}` on restJson1).
    let body = if has_payload_member {
        match payload.into_inner() {
            Some(payload) => payload.bytes,
            None if plan.unset_structure_payload_is_document
                && schema.members().iter().any(|m| {
                    m.http_payload().is_some() && m.shape_type() == aws_smithy_schema::ShapeType::Structure
                }) =>
            {
                let mut serializer = codec.create_serializer();
                serializer.write_struct(schema, &EmptyDocument(schema))?;
                serializer.finish().into()
            }
            None => bytes::Bytes::new(),
        }
    } else {
        body
    };

    Ok(ResponseParts {
        body,
        headers: headers.into_inner(),
        status: status.get(),
    })
}

pub(crate) fn has_output_body_members(schema: &Schema<'_>, bindings: ResponseBindings) -> bool {
    if bindings == ResponseBindings::BodyOnly {
        return !schema.members().is_empty();
    }
    schema
        .members()
        .iter()
        .any(|m| m.http_header().is_none() && m.http_prefix_headers().is_none() && m.http_response_code().is_none())
}

/// Writes no members of the given struct: the codec's empty document.
struct EmptyDocument<'a>(&'a Schema<'a>);

impl SerializableStruct for EmptyDocument<'_> {
    fn schema(&self) -> &Schema<'_> {
        self.0
    }

    fn serialize_members(&self, _: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        Ok(())
    }
}

/// A captured `@httpPayload` member value.
struct CapturedPayload {
    bytes: bytes::Bytes,
}

/// Wrapper diverting bound top-level members into their sinks while
/// forwarding everything else to the codec body serializer.
struct SplitBindings<'a, C> {
    inner: &'a dyn SerializableStruct,
    codec: &'a C,
    headers: &'a CapturedHeaders,
    // The codec calls `serialize_members` through `&dyn SerializableStruct`; `Cell` lets that
    // shared wrapper capture the optional status without allocation or mutable aliasing.
    status: &'a Cell<Option<u16>>,
    payload: &'a RefCell<Option<CapturedPayload>>,
    plan: &'a CompiledResponsePlan,
    // XML may walk members once for attributes and again for child elements.
    // Capture bindings on the first walk, and let every walk forward body members.
    capture_bindings: Cell<bool>,
}

impl<C: Codec> SerializableStruct for SplitBindings<'_, C> {
    fn schema(&self) -> &Schema<'_> {
        self.inner.schema()
    }

    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        let mut splitter = ResponseBindingSplitter {
            body: serializer,
            codec: self.codec,
            headers: self.headers,
            status: self.status,
            payload: self.payload,
            payload_mode: false,
            capture_bindings: self.capture_bindings.replace(false),
            plan: self.plan,
        };
        self.inner.serialize_members(&mut splitter)
    }
}

/// A body sink: discard ordinary members when only the response head is needed,
/// or reject them when an explicit payload forbids other body members.
struct NoBodySerializer {
    discard: bool,
}

macro_rules! no_body_writes {
    ($($method:ident($($arg:ty),*)),+ $(,)?) => {
        $(
            fn $method(&mut self, _: &Schema<'_>, $(_: $arg),*) -> Result<(), SerdeError> {
                if self.discard {
                    Ok(())
                } else {
                    Err(SerdeError::custom(
                        "a member without a response binding cannot coexist with an @httpPayload member",
                    ))
                }
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
        write_blob(aws_smithy_types::Blob),
        write_timestamp(&DateTime),
        write_document(&Document),
        write_null(),
    }
}

// Serialization errors may reach both logs and response bodies. Redact before constructing
// the error, even when instrumentation's `unredacted-logging` feature is enabled.
fn header_diagnostic(value: &str, sensitive: bool) -> String {
    format!(
        "{:?}",
        if sensitive {
            crate::instrumentation::sensitivity::REDACTED
        } else {
            value
        }
    )
}

fn header_value(formatted: &str, sensitive: bool) -> Result<http::HeaderValue, SerdeError> {
    http::HeaderValue::try_from(formatted).map_err(|err| {
        SerdeError::custom(format!(
            "{} cannot be used as a header value: {err}",
            header_diagnostic(formatted, sensitive)
        ))
    })
}

fn capture_header(
    sink: &CapturedHeaders,
    name: &http::HeaderName,
    formatted: &str,
    sensitive: bool,
) -> Result<(), SerdeError> {
    // Mirror the generated `ser_*_headers` functions: empty string
    // values are skipped rather than sent as empty headers.
    if formatted.is_empty() {
        return Ok(());
    }
    let value = header_value(formatted, sensitive)?;
    sink.borrow_mut().push((name.clone(), value));
    Ok(())
}

/// Formats a timestamp for an HTTP header: `@timestampFormat` if present on
/// the member schema, else `http-date` (the Smithy default for header-bound
/// timestamps).
fn format_header_timestamp(format: HeaderTimestampFormat, value: &DateTime) -> Result<String, SerdeError> {
    use aws_smithy_types::date_time::Format;
    let format = match format {
        HeaderTimestampFormat::EpochSeconds => Format::EpochSeconds,
        HeaderTimestampFormat::DateTime => Format::DateTimeWithOffset,
        HeaderTimestampFormat::HttpDate => Format::HttpDate,
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
    /// callers pass the payload member's TARGET schema, which carries the
    /// framing but not the member's binding traits.
    payload_mode: bool,
    capture_bindings: bool,
    plan: &'a CompiledResponsePlan,
}

impl<C: Codec> ResponseBindingSplitter<'_, C> {
    fn skip_binding(&self, schema: &Schema<'_>) -> bool {
        !self.capture_bindings && !matches!(self.plan.member(schema), ResponseMemberPlan::Body)
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

    fn capture_payload(&self, bytes: impl Into<bytes::Bytes>) {
        *self.payload.borrow_mut() = Some(CapturedPayload { bytes: bytes.into() });
    }
}

macro_rules! split_int {
    ($fn_name:ident, $ty:ty) => {
        fn $fn_name(&mut self, schema: &Schema<'_>, value: $ty) -> Result<(), SerdeError> {
            if self.skip_binding(schema) {
                return Ok(());
            }
            match self.plan.member(schema) {
                ResponseMemberPlan::Status => self.capture_status(value as i64),
                ResponseMemberPlan::Header { name, sensitive, .. } => {
                    let mut encoder = aws_smithy_types::primitive::Encoder::from(value);
                    capture_header(self.headers, name, encoder.encode(), *sensitive)
                }
                _ => self.body.$fn_name(schema, value),
            }
        }
    };
}

macro_rules! split_scalar {
    ($fn_name:ident, $ty:ty) => {
        fn $fn_name(&mut self, schema: &Schema<'_>, value: $ty) -> Result<(), SerdeError> {
            if self.skip_binding(schema) {
                return Ok(());
            }
            match self.plan.member(schema) {
                ResponseMemberPlan::Header { name, sensitive, .. } => {
                    let mut encoder = aws_smithy_types::primitive::Encoder::from(value);
                    capture_header(self.headers, name, encoder.encode(), *sensitive)
                }
                _ => self.body.$fn_name(schema, value),
            }
        }
    };
}

impl<C: Codec> ShapeSerializer for ResponseBindingSplitter<'_, C> {
    fn write_struct(&mut self, schema: &Schema<'_>, value: &dyn SerializableStruct) -> Result<(), SerdeError> {
        if self.skip_binding(schema) {
            return Ok(());
        }
        if self.payload_mode {
            // Structure/union payload: the payload member's own framing IS
            // the body — serialize it standalone through the codec.
            let mut serializer = self.codec.create_serializer();
            serializer.write_struct(schema, value)?;
            self.capture_payload(serializer.finish());
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
        if self.skip_binding(schema) {
            return Ok(());
        }
        if let ResponseMemberPlan::Header {
            name,
            media_type,
            timestamp_format,
            sensitive,
            ..
        } = self.plan.member(schema)
        {
            // Each element becomes its own header value under the same name.
            // Formatting also uses element traits inherited from its target.
            let mut collector = HeaderListCollector {
                sink: self.headers,
                name,
                media_type: *media_type,
                timestamp_format: *timestamp_format,
                sensitive: *sensitive,
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
        if self.skip_binding(schema) {
            return Ok(());
        }
        if let ResponseMemberPlan::PrefixHeaders {
            prefix,
            key_sensitive,
            value_sensitive,
        } = self.plan.member(schema)
        {
            let mut collector = PrefixHeaderCollector {
                prefix,
                sink: self.headers,
                pending_key: None,
                key_sensitive: *key_sensitive,
                value_sensitive: *value_sensitive,
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

    fn write_big_integer(&mut self, schema: &Schema<'_>, value: &BigInteger) -> Result<(), SerdeError> {
        if self.skip_binding(schema) {
            return Ok(());
        }
        match self.plan.member(schema) {
            ResponseMemberPlan::Header { name, sensitive, .. } => {
                capture_header(self.headers, name, value.as_ref(), *sensitive)
            }
            _ => self.body.write_big_integer(schema, value),
        }
    }

    fn write_big_decimal(&mut self, schema: &Schema<'_>, value: &BigDecimal) -> Result<(), SerdeError> {
        if self.skip_binding(schema) {
            return Ok(());
        }
        match self.plan.member(schema) {
            ResponseMemberPlan::Header { name, sensitive, .. } => {
                capture_header(self.headers, name, value.as_ref(), *sensitive)
            }
            _ => self.body.write_big_decimal(schema, value),
        }
    }

    fn write_string(&mut self, schema: &Schema<'_>, value: &str) -> Result<(), SerdeError> {
        if self.skip_binding(schema) {
            return Ok(());
        }
        if let ResponseMemberPlan::Payload { raw: true } = self.plan.member(schema) {
            self.capture_payload(bytes::Bytes::copy_from_slice(value.as_bytes()));
            return Ok(());
        }
        if let ResponseMemberPlan::Header {
            name,
            media_type,
            sensitive,
            ..
        } = self.plan.member(schema)
        {
            // `@mediaType` on a header-bound string: base64-encode.
            if *media_type {
                let encoded = aws_smithy_types::base64::encode(value.as_bytes());
                return capture_header(self.headers, name, &encoded, *sensitive);
            }
            return capture_header(self.headers, name, value, *sensitive);
        }
        self.body.write_string(schema, value)
    }

    fn write_blob(&mut self, schema: &Schema<'_>, value: aws_smithy_types::Blob) -> Result<(), SerdeError> {
        if self.skip_binding(schema) {
            return Ok(());
        }
        if let ResponseMemberPlan::Payload { raw: true } = self.plan.member(schema) {
            // `into_bytes` hands over the blob's `Bytes` (a reference count); `into_inner` would
            // copy the whole payload into a `Vec`.
            self.capture_payload(value.into_bytes());
            return Ok(());
        }
        if let ResponseMemberPlan::Header { name, sensitive, .. } = self.plan.member(schema) {
            return capture_header(
                self.headers,
                name,
                &aws_smithy_types::base64::encode(value.as_ref()),
                *sensitive,
            );
        }
        self.body.write_blob(schema, value)
    }

    fn write_timestamp(&mut self, schema: &Schema<'_>, value: &DateTime) -> Result<(), SerdeError> {
        if self.skip_binding(schema) {
            return Ok(());
        }
        match self.plan.member(schema) {
            ResponseMemberPlan::Header {
                name,
                timestamp_format,
                sensitive,
                ..
            } => {
                let formatted = format_header_timestamp(*timestamp_format, value)?;
                capture_header(self.headers, name, &formatted, *sensitive)
            }
            _ => self.body.write_timestamp(schema, value),
        }
    }

    fn write_document(&mut self, schema: &Schema<'_>, value: &Document) -> Result<(), SerdeError> {
        if self.skip_binding(schema) {
            return Ok(());
        }
        if self.payload_mode {
            // The document VALUE is the body: serialize against the prelude
            // document schema, not the member schema — a member schema would
            // make the codec emit a `"memberName":` key fragment.
            let mut serializer = self.codec.create_serializer();
            serializer.write_document(&aws_smithy_schema::prelude::DOCUMENT, value)?;
            self.capture_payload(serializer.finish());
            return Ok(());
        }
        self.body.write_document(schema, value)
    }

    fn write_null(&mut self, schema: &Schema<'_>) -> Result<(), SerdeError> {
        if !matches!(self.plan.member(schema), ResponseMemberPlan::Body) {
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
    name: &'a http::HeaderName,
    media_type: bool,
    timestamp_format: HeaderTimestampFormat,
    sensitive: bool,
}

macro_rules! collect_scalar {
    ($fn_name:ident, $ty:ty) => {
        fn $fn_name(&mut self, _schema: &Schema<'_>, value: $ty) -> Result<(), SerdeError> {
            let mut encoder = aws_smithy_types::primitive::Encoder::from(value);
            capture_header(self.sink, self.name, encoder.encode(), self.sensitive)
        }
    };
}

impl ShapeSerializer for HeaderListCollector<'_> {
    fn write_struct(&mut self, _schema: &Schema<'_>, _value: &dyn SerializableStruct) -> Result<(), SerdeError> {
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
        Err(SerdeError::custom("maps cannot appear in an @httpHeader-bound list"))
    }

    collect_scalar!(write_boolean, bool);
    collect_scalar!(write_byte, i8);
    collect_scalar!(write_short, i16);
    collect_scalar!(write_integer, i32);
    collect_scalar!(write_long, i64);
    collect_scalar!(write_float, f32);
    collect_scalar!(write_double, f64);

    fn write_big_integer(&mut self, _schema: &Schema<'_>, value: &BigInteger) -> Result<(), SerdeError> {
        capture_header(self.sink, self.name, value.as_ref(), self.sensitive)
    }

    fn write_big_decimal(&mut self, _schema: &Schema<'_>, value: &BigDecimal) -> Result<(), SerdeError> {
        capture_header(self.sink, self.name, value.as_ref(), self.sensitive)
    }

    fn write_string(&mut self, schema: &Schema<'_>, value: &str) -> Result<(), SerdeError> {
        // Legacy encodes media-typed strings before emitting each list element.
        // Base64 has no list delimiters to quote; capture_header also skips empty values.
        if self.media_type || schema.media_type().is_some() {
            let encoded = aws_smithy_types::base64::encode(value.as_bytes());
            return capture_header(self.sink, self.name, &encoded, self.sensitive);
        }
        // Elements of a header-bound list are quoted when they contain `,` or
        // `"` (RFC 9110 list syntax) — mirroring the generated
        // serializers' `quote_header_value` usage.
        let quoted = aws_smithy_http::header::quote_header_value(value);
        capture_header(self.sink, self.name, quoted.as_ref(), self.sensitive)
    }

    fn write_blob(&mut self, _schema: &Schema<'_>, value: aws_smithy_types::Blob) -> Result<(), SerdeError> {
        capture_header(
            self.sink,
            self.name,
            &aws_smithy_types::base64::encode(value.as_ref()),
            self.sensitive,
        )
    }

    fn write_timestamp(&mut self, _schema: &Schema<'_>, value: &DateTime) -> Result<(), SerdeError> {
        let formatted = format_header_timestamp(self.timestamp_format, value)?;
        capture_header(self.sink, self.name, &formatted, self.sensitive)
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
    key_sensitive: bool,
    value_sensitive: bool,
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
                // An empty value is still sent (`x-p-empty:`), as legacy does: unlike bound
                // headers, prefix headers have no skip-empty rule.
                let full_name = format!("{}{}", self.prefix, key);
                let name = http::HeaderName::try_from(full_name.as_str()).map_err(|err| {
                    SerdeError::custom(format!(
                        "{} cannot be used as a header name: {err}",
                        header_diagnostic(&full_name, self.key_sensitive)
                    ))
                })?;
                let header_value = header_value(value, self.value_sensitive)?;
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
        write_blob(aws_smithy_types::Blob),
        write_timestamp(&DateTime),
        write_document(&Document),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::protocol::response::resolve_status;
    use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
    use aws_smithy_schema::traits::HttpTrait;
    use aws_smithy_schema::ShapeId;
    use aws_smithy_schema::ShapeType;

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
    static OUT_MEMBERS: [&Schema<'static>; 4] = [&CODE_MEMBER, &HDR_MEMBER, &META_MEMBER, &BODY_MEMBER];
    static OUT_SCHEMA: Schema<'static> = Schema::new_struct(
        ShapeId::from_parts("test#Out", "test", "Out"),
        ShapeType::Structure,
        &OUT_MEMBERS,
    )
    .with_http(HttpTrait::new("POST", "/op", Some(201)));

    struct Out;
    impl SerializableStruct for Out {
        fn schema(&self) -> &Schema<'_> {
            &OUT_SCHEMA
        }

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
    fn response_bindings_and_status() {
        // REST path: @httpHeader and @httpPrefixHeaders divert to headers,
        // @httpResponseCode is captured (never in the body), the rest is the
        // codec body.
        let codec = json_codec();
        let split = serialize_response_parts(
            &codec,
            &OUT_SCHEMA,
            &Out,
            ResponseBindings::Rest,
            ResponseValueKind::ModeledError,
        )
        .unwrap();
        assert_eq!(split.status, Some(202));
        assert_eq!(String::from_utf8(split.body.to_vec()).unwrap(), r#"{"msg":"hello"}"#);
        let headers: Vec<(String, String)> = split
            .headers
            .iter()
            .map(|(n, v)| (n.to_string(), v.to_str().unwrap().to_string()))
            .collect();
        assert!(headers.contains(&("x-hdr".to_string(), "hval".to_string())));
        assert!(headers.contains(&("x-meta-color".to_string(), "red".to_string())));

        // Status resolution: captured @httpResponseCode, else @http code,
        // else 200.
        assert_eq!(resolve_status(split.status, OUT_SCHEMA.http()), 202);
        assert_eq!(resolve_status(None, OUT_SCHEMA.http()), 201);
        static PLAIN: Schema<'static> =
            Schema::new(ShapeId::from_parts("test#Plain", "test", "Plain"), ShapeType::Structure);
        assert_eq!(resolve_status(None, PLAIN.http()), 200);

        // RPC path: everything, bound or not, goes to the body.
        let split = serialize_response_parts(
            &codec,
            &OUT_SCHEMA,
            &Out,
            ResponseBindings::BodyOnly,
            ResponseValueKind::ModeledError,
        )
        .unwrap();
        assert_eq!(split.status, None);
        assert!(split.headers.is_empty());
        let body = String::from_utf8(split.body.to_vec()).unwrap();
        assert!(body.contains("\"code\":202"));
        assert!(body.contains("\"hdr\":\"hval\""));
        assert!(body.contains("\"msg\":\"hello\""));
    }

    #[test]
    fn xml_attribute_passes_capture_each_binding_and_collection_callback_once() {
        static ITEMS: Schema<'static> = Schema::new_member(
            ShapeId::from_parts("test#XmlOutput$items", "test", "XmlOutput"),
            ShapeType::List,
            "items",
            4,
        )
        .with_list_member(&aws_smithy_schema::prelude::STRING)
        .with_http_header("x-items");
        static ATTRIBUTE: Schema<'static> = Schema::new_member(
            ShapeId::from_parts("test#XmlOutput$attr", "test", "XmlOutput"),
            ShapeType::String,
            "attr",
            5,
        )
        .with_xml_attribute();
        static SCHEMA: Schema<'static> = Schema::new_struct(
            ShapeId::from_parts("test#XmlOutput", "test", "XmlOutput"),
            ShapeType::Structure,
            &[
                &CODE_MEMBER,
                &HDR_MEMBER,
                &META_MEMBER,
                &BODY_MEMBER,
                &ITEMS,
                &ATTRIBUTE,
            ],
        );
        #[derive(Default)]
        struct XmlOutput {
            walks: Cell<usize>,
            lists: Cell<usize>,
            maps: Cell<usize>,
        }
        impl SerializableStruct for XmlOutput {
            fn schema(&self) -> &Schema<'_> {
                &SCHEMA
            }

            fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                self.walks.set(self.walks.get() + 1);
                s.write_integer(&CODE_MEMBER, 200 + self.walks.get() as i32)?;
                s.write_string(&HDR_MEMBER, "scalar")?;
                s.write_list(&ITEMS, &|s| {
                    self.lists.set(self.lists.get() + 1);
                    s.write_string(&aws_smithy_schema::prelude::STRING, "one")?;
                    s.write_string(&aws_smithy_schema::prelude::STRING, "two")
                })?;
                s.write_map(&META_MEMBER, &|s| {
                    self.maps.set(self.maps.get() + 1);
                    s.write_string(&META_MEMBER, "color")?;
                    s.write_string(&META_MEMBER, "red")
                })?;
                s.write_string(&ATTRIBUTE, "attr")?;
                s.write_string(&BODY_MEMBER, "body")
            }
        }
        let codec = aws_smithy_xml::codec::XmlCodec::default();
        let plan = CompiledResponsePlan::compile(&SCHEMA, ResponseBindings::Rest, ResponseValueKind::ModeledError);
        for compiled in [false, true] {
            let value = XmlOutput::default();
            let split = if compiled {
                serialize_response_parts_compiled(&codec, &SCHEMA, &value, &plan)
            } else {
                serialize_response_parts(
                    &codec,
                    &SCHEMA,
                    &value,
                    ResponseBindings::Rest,
                    ResponseValueKind::ModeledError,
                )
            }
            .unwrap();
            assert_eq!(split.headers.len(), 4);
            assert_eq!(split.status, Some(201));
            assert_eq!(value.walks.get(), 2, "XML still needs its two body passes");
            assert_eq!(value.lists.get(), 1);
            assert_eq!(value.maps.get(), 1);
            assert_eq!(
                split.body.as_ref(),
                b"<XmlOutput attr=\"attr\"><msg>body</msg></XmlOutput>"
            );
        }
    }

    static EMPTY_OUT_SCHEMA: Schema<'static> = Schema::new_struct(
        ShapeId::from_parts("test#EmptyOut", "test", "EmptyOut"),
        ShapeType::Structure,
        &[],
    )
    .with_http(HttpTrait::new("POST", "/empty", Some(204)));

    struct EmptyOut;
    impl SerializableStruct for EmptyOut {
        fn schema(&self) -> &Schema<'_> {
            &EMPTY_OUT_SCHEMA
        }

        fn serialize_members(&self, _s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            Ok(())
        }
    }

    static MODELED_EMPTY_OUT_SCHEMA: Schema<'static> = Schema::new_struct(
        ShapeId::from_parts("test#ModeledEmptyOut", "test", "ModeledEmptyOut"),
        ShapeType::Structure,
        &[],
    )
    .with_original_name("ModeledEmptyOut");

    const OUTPUT: ResponseValueKind = ResponseValueKind::OperationOutput { empty_document: true };

    fn output_plan(schema: &Schema<'_>) -> CompiledResponsePlan {
        CompiledResponsePlan::compile(schema, ResponseBindings::Rest, OUTPUT)
    }

    #[test]
    fn empty_success_outputs_are_not_codec_serialized() {
        let codec = json_codec();

        let split =
            serialize_response_parts(&codec, &EMPTY_OUT_SCHEMA, &EmptyOut, ResponseBindings::Rest, OUTPUT).unwrap();
        assert!(split.body.is_empty());
        assert_eq!(resolve_status(split.status, EMPTY_OUT_SCHEMA.http()), 204);

        let split =
            serialize_response_parts(&codec, &EMPTY_OUT_SCHEMA, &EmptyOut, ResponseBindings::BodyOnly, OUTPUT).unwrap();
        assert!(split.body.is_empty());

        // A user-modeled empty output is an empty document when the protocol asks for one, on
        // both binding styles, and an empty body when it does not (restXml).
        for bindings in [ResponseBindings::Rest, ResponseBindings::BodyOnly] {
            let split =
                serialize_response_parts(&codec, &MODELED_EMPTY_OUT_SCHEMA, &EmptyOut, bindings, OUTPUT).unwrap();
            assert_eq!(String::from_utf8(split.body.to_vec()).unwrap(), "{}");
        }
        let split = serialize_response_parts(
            &codec,
            &MODELED_EMPTY_OUT_SCHEMA,
            &EmptyOut,
            ResponseBindings::Rest,
            ResponseValueKind::OperationOutput { empty_document: false },
        )
        .unwrap();
        assert!(split.body.is_empty());

        // Empty modeled errors still go through the normal codec path.
        let split = serialize_response_parts(
            &codec,
            &EMPTY_OUT_SCHEMA,
            &EmptyOut,
            ResponseBindings::Rest,
            ResponseValueKind::ModeledError,
        )
        .unwrap();
        assert_eq!(String::from_utf8(split.body.to_vec()).unwrap(), "{}");
    }

    static EVENTS_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#StreamOut$events", "test", "StreamOut"),
        ShapeType::Union,
        "events",
        0,
    )
    .with_http_payload()
    .with_streaming();
    static STREAM_CODE_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#StreamOut$code", "test", "StreamOut"),
        ShapeType::Integer,
        "code",
        1,
    )
    .with_http_response_code();
    static STREAM_HDR_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#StreamOut$hdr", "test", "StreamOut"),
        ShapeType::String,
        "hdr",
        2,
    )
    .with_http_header("x-hdr");
    static STREAM_MEMBERS: [&Schema<'static>; 3] = [&EVENTS_MEMBER, &STREAM_CODE_MEMBER, &STREAM_HDR_MEMBER];
    static STREAM_OUT_SCHEMA: Schema<'static> = Schema::new_struct(
        ShapeId::from_parts("test#StreamOut", "test", "StreamOut"),
        ShapeType::Structure,
        &STREAM_MEMBERS,
    );

    struct StreamOut;
    impl SerializableStruct for StreamOut {
        fn schema(&self) -> &Schema<'_> {
            &STREAM_OUT_SCHEMA
        }

        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            // Generated outputs skip their streaming member.
            s.write_integer(&STREAM_CODE_MEMBER, 202)?;
            s.write_string(&STREAM_HDR_MEMBER, "hval")
        }
    }

    #[test]
    fn streaming_outputs_write_bindings_only() {
        let codec = json_codec();
        let split = serialize_response_parts(
            &codec,
            &STREAM_OUT_SCHEMA,
            &StreamOut,
            ResponseBindings::Rest,
            ResponseValueKind::StreamingOutput,
        )
        .unwrap();
        assert!(split.body.is_empty());
        assert_eq!(split.status, Some(202));
        assert!(split.headers.iter().any(|(name, _)| name == "x-hdr"));

        // Body-only protocols write nothing at all for the head.
        let split = serialize_response_parts(
            &codec,
            &STREAM_OUT_SCHEMA,
            &StreamOut,
            ResponseBindings::BodyOnly,
            ResponseValueKind::StreamingOutput,
        )
        .unwrap();
        assert!(split.body.is_empty());
        assert!(split.headers.is_empty());
    }

    #[test]
    fn response_plan_is_compiled_from_top_level_bindings() {
        assert_eq!(output_plan(&EMPTY_OUT_SCHEMA).strategy, ResponseStrategy::Empty);
        assert_eq!(
            output_plan(&MODELED_EMPTY_OUT_SCHEMA).strategy,
            ResponseStrategy::CodecBody
        );
        assert_eq!(output_plan(&OUT_SCHEMA).strategy, ResponseStrategy::SplitBody);
        assert_eq!(output_plan(&BLOB_OUT_SCHEMA).strategy, ResponseStrategy::Payload);

        static ONLY_HEADER: Schema<'static> = Schema::new_member(
            ShapeId::from_parts("test#HeaderOnly$hdr", "test", "HeaderOnly"),
            ShapeType::String,
            "hdr",
            0,
        )
        .with_http_header("x-hdr");
        static ONLY_BODY: Schema<'static> = Schema::new_member(
            ShapeId::from_parts("test#BodyOnly$msg", "test", "BodyOnly"),
            ShapeType::String,
            "msg",
            0,
        );
        static HEADER_MEMBERS: [&Schema<'static>; 1] = [&ONLY_HEADER];
        static HEADER_ONLY: Schema<'static> = Schema::new_struct(
            ShapeId::from_parts("test#HeaderOnly", "test", "HeaderOnly"),
            ShapeType::Structure,
            &HEADER_MEMBERS,
        );
        static BODY_MEMBERS: [&Schema<'static>; 1] = [&ONLY_BODY];
        static BODY_ONLY: Schema<'static> = Schema::new_struct(
            ShapeId::from_parts("test#BodyOnly", "test", "BodyOnly"),
            ShapeType::Structure,
            &BODY_MEMBERS,
        );
        assert_eq!(output_plan(&HEADER_ONLY).strategy, ResponseStrategy::BindingsOnly);
        assert_eq!(output_plan(&BODY_ONLY).strategy, ResponseStrategy::CodecBody);
    }

    #[test]
    #[should_panic(expected = "invalid @httpHeader name `not a header`")]
    fn invalid_header_names_fail_during_response_plan_compilation() {
        static INVALID_HEADER: Schema<'static> = Schema::new_member(
            ShapeId::from_parts("test#InvalidHeader$value", "test", "InvalidHeader"),
            ShapeType::String,
            "value",
            0,
        )
        .with_http_header("not a header");
        static MEMBERS: [&Schema<'static>; 1] = [&INVALID_HEADER];
        static OUTPUT: Schema<'static> = Schema::new_struct(
            ShapeId::from_parts("test#InvalidHeader", "test", "InvalidHeader"),
            ShapeType::Structure,
            &MEMBERS,
        );

        let _ = output_plan(&OUTPUT);
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
        fn schema(&self) -> &Schema<'_> {
            &BLOB_OUT_SCHEMA
        }

        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            if let Some(bytes) = &self.0 {
                s.write_blob(&BLOB_PAYLOAD_MEMBER, aws_smithy_types::Blob::new(bytes.clone()))?;
            }
            Ok(())
        }
    }

    /// Regression (proof 31): a blob payload moves into the response body without a copy. The
    /// body must be the blob's own memory, not a copy made by `Blob::into_inner`.
    #[test]
    fn blob_payload_body_shares_the_blob_bytes() {
        // Like generated output structs: holds a `Blob` and clones it (a reference count).
        struct SharedBlobOut(aws_smithy_types::Blob);
        impl SerializableStruct for SharedBlobOut {
            fn schema(&self) -> &Schema<'_> {
                &BLOB_OUT_SCHEMA
            }

            fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                s.write_blob(&BLOB_PAYLOAD_MEMBER, self.0.clone())
            }
        }

        let payload = bytes::Bytes::from(vec![7u8; 1 << 16]);
        let split = serialize_response_parts(
            &json_codec(),
            &BLOB_OUT_SCHEMA,
            &SharedBlobOut(aws_smithy_types::Blob::from_maybe_shared(payload.clone())),
            ResponseBindings::Rest,
            ResponseValueKind::ModeledError,
        )
        .unwrap();
        assert_eq!(split.body, payload);
        assert_eq!(split.body.as_ptr(), payload.as_ptr(), "the payload was copied");
    }

    #[test]
    fn payload_bodies() {
        // Blob payload: raw bytes.
        let codec = json_codec();
        let split = serialize_response_parts(
            &codec,
            &BLOB_OUT_SCHEMA,
            &BlobOut(Some(vec![1, 2, 3])),
            ResponseBindings::Rest,
            ResponseValueKind::ModeledError,
        )
        .unwrap();
        assert_eq!(split.body, vec![1, 2, 3]);

        // Unset payload member: empty body.
        let split = serialize_response_parts(
            &codec,
            &BLOB_OUT_SCHEMA,
            &BlobOut(None),
            ResponseBindings::Rest,
            ResponseValueKind::ModeledError,
        )
        .unwrap();
        assert!(split.body.is_empty());

        // An unset structure payload is the codec's empty document where the protocol asks for
        // one (restJson1's legacy serializer writes `{}`), an empty body elsewhere.
        struct Unset;
        impl SerializableStruct for Unset {
            fn schema(&self) -> &Schema<'_> {
                &STRUCT_OUT_SCHEMA
            }

            fn serialize_members(&self, _: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                Ok(())
            }
        }
        let split =
            serialize_response_parts(&codec, &STRUCT_OUT_SCHEMA, &Unset, ResponseBindings::Rest, OUTPUT).unwrap();
        assert_eq!(String::from_utf8(split.body.to_vec()).unwrap(), "{}");
        let split = serialize_response_parts(
            &codec,
            &STRUCT_OUT_SCHEMA,
            &Unset,
            ResponseBindings::Rest,
            ResponseValueKind::OperationOutput { empty_document: false },
        )
        .unwrap();
        assert!(split.body.is_empty());

        // Structure payload (written against its TARGET schema, the codegen
        // convention): the body is the codec document of that member alone.
        let split = serialize_response_parts(
            &codec,
            &STRUCT_OUT_SCHEMA,
            &StructOut,
            ResponseBindings::Rest,
            ResponseValueKind::ModeledError,
        )
        .unwrap();
        assert_eq!(String::from_utf8(split.body.to_vec()).unwrap(), r#"{"f":"v"}"#);
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
        fn schema(&self) -> &Schema<'_> {
            &STRUCT_OUT_SCHEMA
        }

        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            struct Nested;
            impl SerializableStruct for Nested {
                fn schema(&self) -> &Schema<'_> {
                    &STRUCT_PAYLOAD_TARGET
                }

                fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
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
