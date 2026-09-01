/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The server-side protocol trait: schema-driven request deserialization and
//! output/error serialization (plan 2a).
//!
//! [`ServerProtocol`] is implemented once per protocol on the existing
//! zero-sized protocol markers ([`RestJson1`], [`AwsJson1_0`], [`AwsJson1_1`],
//! [`RpcV2Cbor`], [`RestXml`]). Protocols are types: every member is an
//! associated function — server protocol dispatch is fully static (the
//! multi-protocol router nests protocol services monomorphized over their
//! marker; protocols never come from config), and per-service protocol facts
//! such as `@xmlNamespace` ride on schemas, not on protocol values.
//!
//! The three verbs mirror the client's `ClientProtocolInner`
//! (`serialize_request`↔[`deserialize_request`](ServerProtocol::deserialize_request),
//! `deserialize_response`↔[`serialize_response`](ServerProtocol::serialize_response),
//! `deserialize_error_response`↔[`serialize_error`](ServerProtocol::serialize_error)),
//! diverging where server semantics demand: no error correction, unknown
//! union variants rejected, constraint failures produce the modeled
//! validation error.
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
//! Serializers never detect errors; call sites declare them by calling
//! [`ServerProtocol::serialize_error`].

use std::borrow::Cow;
use std::sync::LazyLock;

use aws_smithy_schema::codec::Codec;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer, ShapeSerializer};
use aws_smithy_schema::{Schema, ShapeId, ShapeType};

use crate::body::BoxBody;
use crate::deserialize::{DeserializableShape, DeserializeError};
use crate::extension::ModeledErrorExtension;
use crate::modeled_error::HttpModeledError;
use crate::protocol::request_bindings::{EmptyStructDeserializer, RestRequestDeserializer};
use crate::protocol::response_bindings::{
    resolve_status, serialize_split, BodyKind, SplitResponse,
};
use crate::rejection::MissingContentTypeReason;
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

/// Implemented on each protocol marker. One impl per protocol; all dispatch
/// is static. Associated functions only — protocols have no instances.
pub trait ServerProtocol: ProtocolShape + 'static {
    /// Body codec. Also the event-stream frame-payload codec — the client
    /// needs a dyn `payload_codec()` accessor because its protocol is a
    /// runtime value; server dispatch is static, so `Self::Codec` serves
    /// both.
    ///
    /// Associated type, not `DynCodec`: `FinishSerializer::finish` is not
    /// object-safe, and no protocol-erased call site exists server-side.
    type Codec: Codec + 'static;

    /// The rejection type for [`deserialize_request`](Self::deserialize_request)
    /// failures — the protocol's `RequestRejection` enum. Wire-level failures
    /// map to malformed-request variants (protocol 4xx); constraint
    /// violations map to `ConstraintViolation`, carrying the modeled
    /// validation error serialized once at the protocol boundary.
    type RequestRejection: std::fmt::Debug + std::fmt::Display + Send + From<DeserializeError>;

    /// Returns this protocol's codec.
    fn codec() -> &'static Self::Codec;

    /// Request path: reads `@http` bindings off the operation input schema
    /// (labels from the URI matched against `schema.http().uri()`, query
    /// strings, headers, `@httpPayload`, body via `Self::Codec`), presenting
    /// ONE composite deserializer to the generated walker `T`.
    ///
    /// Distinguishes malformed-request failures (protocol 4xx) from
    /// constraint violations (the modeled validation error) through
    /// `Self::RequestRejection`.
    fn deserialize_request<T: DeserializableShape>(
        schema: &Schema<'_>,
        output_schema: &Schema<'_>,
        parts: &http::request::Parts,
        body: &[u8],
    ) -> Result<T, Self::RequestRejection> {
        Self::with_request_deserializer(schema, output_schema, parts, body, |deserializer| {
            T::deserialize(deserializer)
        })
    }

    /// Like [`deserialize_request`](Self::deserialize_request), but hands the
    /// composite deserializer to `f` instead of driving `T::deserialize`.
    /// Event-stream operation glue uses this seam to walk into the input's
    /// internal BUILDER, attach the frame receiver, and only then `build()`
    /// (the stream member is `@required`-equivalent).
    fn with_request_deserializer<R>(
        schema: &Schema<'_>,
        output_schema: &Schema<'_>,
        parts: &http::request::Parts,
        body: &[u8],
        f: impl FnOnce(&mut dyn ShapeDeserializer) -> Result<R, DeserializeError>,
    ) -> Result<R, Self::RequestRejection>;

    /// Success path. Status: `@httpResponseCode` member if bound and set,
    /// else `schema.http().code()`, else `200`. REST protocols honor
    /// response bindings read off member schemas; RPC protocols serialize
    /// body-only.
    ///
    /// Serialization failure logs via `tracing` and falls back to the
    /// protocol's `RuntimeError::Serialization` response, preserving the
    /// legacy generated `IntoResponse` contract.
    fn serialize_response(
        schema: &Schema<'_>,
        output: &dyn SerializableStruct,
    ) -> http::Response<BoxBody>;

    /// Error path. Status from [`HttpModeledError::status_code`],
    /// discriminator framing per protocol, header-bound members split out of
    /// the body on the REST protocols. Same internal fallback as
    /// [`serialize_response`](Self::serialize_response).
    fn serialize_error(error: &dyn HttpModeledError) -> http::Response<BoxBody>;
}

/// Event-stream capability subtrait (Option B of
/// `specs/eventstream-capability-options.md`, kept as the decision record):
/// implemented only by protocols whose Smithy definition declares
/// `eventStreamHttp`. Wiring an event-stream operation to a non-supporting
/// protocol is a compile error at assembly, not a runtime failure. Frame glue
/// and event-stream operation impls bound on `P: EventStreamProtocol`;
/// ordinary operations stay `P: ServerProtocol`. Bounds never reach
/// user-facing signatures (concrete-marker instantiation).
pub trait EventStreamProtocol: ServerProtocol {
    /// Frame-level `:content-type` for event payloads (json:
    /// `application/json`, cbor: `application/cbor`). Fixes the client's
    /// baked-literal leak.
    const EVENT_PAYLOAD_CONTENT_TYPE: &'static str;

    /// HTTP-level `Content-Type` of the streaming response. NOT uniform:
    /// restJson1/restXml/rpcv2Cbor declare
    /// `application/vnd.amazon.eventstream`; awsJson keeps
    /// `application/x-amz-json-1.x` (`AwsJson.kt:93` — response content type
    /// equals request content type unconditionally, no
    /// `eventStreamContentType` override).
    const EVENT_STREAM_HTTP_CONTENT_TYPE: &'static str;

    /// RPC protocols frame initial-request/initial-response messages; REST
    /// protocols put the prelude in HTTP and the body is frames-only.
    /// (Per-operation conditions — a non-stream `DOCUMENT` member for the
    /// request direction, the `alwaysSendEventStreamInitialResponse` setting
    /// for the response direction — live in generated glue, not here.)
    const FRAMES_INITIAL_MESSAGES: bool;
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
// Shared response assembly
// ============================================================================

/// Assembles a response from a [`SplitResponse`]: status, content type per
/// [`BodyKind`], captured binding headers, content-length.
///
/// Mirrors the legacy generated `ser_*_http_response` functions, which stamp
/// content-type, binding headers, and content-length via
/// `set_response_header_if_absent` (the headers cannot already be present on
/// a fresh builder, so plain insertion is equivalent).
fn assemble_response(
    split: SplitResponse,
    status: u16,
    codec_content_type: &'static str,
) -> Result<http::Response<BoxBody>, SerdeError> {
    let mut builder = http::Response::builder().status(
        http::StatusCode::from_u16(status).unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR),
    );
    let content_type: Option<Cow<'_, str>> = match &split.kind {
        BodyKind::Codec => Some(Cow::Borrowed(codec_content_type)),
        BodyKind::Raw { content_type } => Some(Cow::Borrowed(content_type.as_str())),
        BodyKind::Empty => None,
    };
    if let Some(content_type) = content_type {
        builder = builder.header(http::header::CONTENT_TYPE, content_type.as_ref());
    }
    for (name, value) in split.headers {
        builder = builder.header(name, value);
    }
    builder = builder.header(http::header::CONTENT_LENGTH, split.body.len());
    builder
        .body(crate::body::to_boxed(split.body))
        .map_err(|err| SerdeError::custom(format!("failed to build response: {err}")))
}

/// Finishes an error response: inserts the [`ModeledErrorExtension`],
/// preserving the legacy generated `IntoResponse` behavior.
fn stamp_error_extension(
    mut response: http::Response<BoxBody>,
    error_name: &str,
) -> http::Response<BoxBody> {
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
// Shared request-side helpers
// ============================================================================

/// What the request's `Content-Type` header must look like for an input
/// schema, mirroring the legacy generated checks.
enum ExpectedContentType<'s> {
    /// No check at all (all members bound to non-body locations, or a blob
    /// `@httpPayload` without `@mediaType` — the legacy generator skips the
    /// check for those).
    Skip,
    /// The header must be absent (`serverContentTypeCheckNoModeledInput`
    /// protocols, operations without modeled input). Checked even on an
    /// empty body.
    Absent,
    /// The header must match — but only when the request body is non-empty
    /// (the legacy `if !bytes.is_empty()` gate; see smithy-lang/smithy#2327).
    Expect(Cow<'s, str>),
}

/// Computes the `Content-Type` expectation for this input schema: the payload
/// member's content type when `@httpPayload` is modeled, the protocol's codec
/// content type when unbound (body) members exist, absence for
/// no-modeled-input operations on protocols that demand it.
fn expected_request_content_type<'s>(
    schema: &'s Schema<'s>,
    codec_content_type: &'static str,
    check_absent_when_no_input: bool,
) -> ExpectedContentType<'s> {
    if let Some(payload) = schema.members().iter().find(|m| m.http_payload().is_some()) {
        let media_type = payload.media_type().map(|m| Cow::Borrowed(m.value()));
        return match (payload.shape_type(), media_type) {
            // Legacy skips the check for blob payloads without @mediaType.
            (ShapeType::Blob, None) => ExpectedContentType::Skip,
            (ShapeType::Blob, Some(media)) => ExpectedContentType::Expect(media),
            (ShapeType::String, media) => {
                ExpectedContentType::Expect(media.unwrap_or(Cow::Borrowed("text/plain")))
            }
            _ => ExpectedContentType::Expect(Cow::Borrowed(codec_content_type)),
        };
    }
    if schema.members().is_empty() {
        // `original_name` is transcribed from the synthetic input trait's
        // original id, which exists exactly when the operation had
        // user-modeled input — the legacy `hadUserModeledOperationInput`
        // signal. A user-modeled EMPTY input struct therefore skips the
        // absence check (RestJsonEmptyInputAndEmptyOutput accepts a
        // `Content-Type` header).
        return if check_absent_when_no_input && schema.original_name().is_none() {
            ExpectedContentType::Absent
        } else {
            ExpectedContentType::Skip
        };
    }
    let has_unbound_members = schema.members().iter().any(|m| {
        m.http_header().is_none()
            && m.http_query().is_none()
            && m.http_label().is_none()
            && m.http_prefix_headers().is_none()
            && m.http_query_params().is_none()
    });
    if has_unbound_members {
        ExpectedContentType::Expect(Cow::Borrowed(codec_content_type))
    } else {
        ExpectedContentType::Skip
    }
}

/// Checks the request `Content-Type` header against the expected value,
/// mirroring [`super::content_type_header_classifier_smithy`] for
/// `http::HeaderMap` and non-`'static` expected values.
#[allow(clippy::result_large_err)]
fn check_content_type(
    headers: &http::HeaderMap,
    expected: Option<&str>,
) -> Result<(), MissingContentTypeReason> {
    let actual = match headers.get(http::header::CONTENT_TYPE) {
        Some(value) => Some(
            value
                .to_str()
                .map_err(|_| MissingContentTypeReason::UnexpectedMimeType {
                    expected_mime: expected.and_then(|e| e.parse().ok()),
                    found_mime: None,
                })?,
        ),
        None => None,
    };
    let parse = |s: &str| {
        s.parse::<mime::Mime>()
            .map_err(MissingContentTypeReason::MimeParseError)
    };
    match (actual, expected) {
        (None, None) => Ok(()),
        (None, Some(expected)) => Err(MissingContentTypeReason::UnexpectedMimeType {
            expected_mime: expected.parse().ok(),
            found_mime: None,
        }),
        (Some(actual), None) => Err(MissingContentTypeReason::UnexpectedMimeType {
            expected_mime: None,
            found_mime: Some(parse(actual)?),
        }),
        (Some(actual), Some(expected)) => {
            let found = parse(actual)?;
            if expected != found.essence_str() {
                Err(MissingContentTypeReason::UnexpectedMimeType {
                    expected_mime: expected.parse().ok(),
                    found_mime: Some(found),
                })
            } else {
                Ok(())
            }
        }
    }
}

/// Runs the content-type expectation against the request, mirroring the
/// legacy gating: `Expect` only applies to non-empty bodies; `Absent` is
/// checked unconditionally.
#[allow(clippy::result_large_err)]
fn enforce_content_type(
    expected: ExpectedContentType<'_>,
    parts: &http::request::Parts,
    body: &[u8],
) -> Result<(), MissingContentTypeReason> {
    match expected {
        ExpectedContentType::Skip => Ok(()),
        ExpectedContentType::Absent => check_content_type(&parts.headers, None),
        ExpectedContentType::Expect(content_type) => {
            if body.is_empty() {
                Ok(())
            } else {
                check_content_type(&parts.headers, Some(content_type.as_ref()))
            }
        }
    }
}

/// The `Content-Type` the operation's response will carry, resolved from the
/// OUTPUT schema — the value the request's `Accept` header is validated
/// against (mirroring the legacy `verifyAcceptHeader`, which used the binding
/// resolver's `responseContentType`):
///
/// - `@httpPayload` member: its `@mediaType`, else `application/octet-stream`
///   for blobs and `text/plain` for strings; codec content type for
///   struct/union/document payloads.
/// - otherwise: the codec content type when any member serializes to the
///   response body; `None` (no Accept check) when none does.
fn expected_response_content_type<'s>(
    output_schema: &'s Schema<'s>,
    codec_content_type: &'static str,
    event_stream_content_type: &'static str,
) -> Option<Cow<'s, str>> {
    // An event-stream output: the response is the frame stream, and the legacy
    // Accept check validated against the event-stream HTTP content type.
    let _ = event_stream_content_type;
    if let Some(payload) = output_schema
        .members()
        .iter()
        .find(|m| m.http_payload().is_some())
    {
        let media_type = payload.media_type().map(|m| m.value());
        return match (payload.shape_type(), media_type) {
            (ShapeType::Blob, Some(media)) => Some(Cow::Borrowed(media)),
            // A blob payload without `@mediaType` may produce any bytes — every
            // `Accept` is satisfiable (RestJsonHttpPayloadTraitsWithBlobAcceptsAllAccepts).
            (ShapeType::Blob, None) => None,
            (ShapeType::String, Some(media)) => Some(Cow::Borrowed(media)),
            (ShapeType::String, None) => Some(Cow::Borrowed("text/plain")),
            _ => Some(Cow::Borrowed(codec_content_type)),
        };
    }
    let has_body_members = output_schema.members().iter().any(|m| {
        m.http_header().is_none()
            && m.http_prefix_headers().is_none()
            && m.http_response_code().is_none()
    });
    has_body_members.then_some(Cow::Borrowed(codec_content_type))
}

/// Validates the request `Accept` header against the response content type
/// resolved from the OUTPUT schema. Returns `false` when the request's
/// `Accept` cannot be satisfied (→ `NotAcceptable`).
fn accept_matches_output(
    headers: &http::HeaderMap,
    output_schema: &Schema<'_>,
    codec_content_type: &'static str,
    event_stream_content_type: &'static str,
) -> bool {
    match expected_response_content_type(output_schema, codec_content_type, event_stream_content_type) {
        Some(expected) => match expected.parse::<mime::Mime>() {
            Ok(mime) => super::accept_header_classifier(headers, &mime),
            // An unparseable modeled @mediaType cannot be validated; accept.
            Err(_) => true,
        },
        None => true,
    }
}

/// The response for an operation with NO user-modeled output (the schema has
/// no `original_name`): an empty body, no codec invocation, and — protocol
/// dependent — no `Content-Type` header. Mirrors the legacy generated
/// serializers, which never opened the codec for synthetic empty outputs.
fn no_modeled_output_response(
    schema: &Schema<'_>,
    content_type: Option<&'static str>,
) -> http::Response<BoxBody> {
    let status = resolve_status(None, schema);
    let mut builder = http::Response::builder().status(
        http::StatusCode::from_u16(status).unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR),
    );
    if let Some(content_type) = content_type {
        builder = builder.header(http::header::CONTENT_TYPE, content_type);
    }
    builder = builder.header(http::header::CONTENT_LENGTH, 0);
    builder
        .body(crate::body::empty())
        .expect("valid status and static headers cannot fail to build")
}

/// The shared REST request path: content-type validation, then the composite
/// binding deserializer driving the generated walker.
fn deserialize_rest_request<Out, F, C, R>(
    codec: &'static C,
    codec_content_type: &'static str,
    check_absent_when_no_input: bool,
    schema: &Schema<'_>,
    parts: &http::request::Parts,
    body: &[u8],
    f: F,
) -> Result<Out, R>
where
    F: FnOnce(&mut dyn ShapeDeserializer) -> Result<Out, DeserializeError>,
    C: Codec,
    R: From<DeserializeError> + From<MissingContentTypeReason>,
{
    let expected =
        expected_request_content_type(schema, codec_content_type, check_absent_when_no_input);
    enforce_content_type(expected, parts, body).map_err(R::from)?;
    let mut deserializer = RestRequestDeserializer::new(codec, parts, body);
    f(&mut deserializer).map_err(R::from)
}

/// The shared RPC request path: content-type validation (non-empty bodies
/// only, per the legacy gate), then body-only deserialization through the
/// codec (an empty body reads as a structure with no members present —
/// `@required` enforcement stays in `build()`).
fn deserialize_rpc_request<Out, F, C, R>(
    codec: &'static C,
    codec_content_type: &'static str,
    schema: &Schema<'_>,
    parts: &http::request::Parts,
    body: &[u8],
    f: F,
) -> Result<Out, R>
where
    F: FnOnce(&mut dyn ShapeDeserializer) -> Result<Out, DeserializeError>,
    C: Codec,
    R: From<DeserializeError> + From<MissingContentTypeReason>,
{
    // An input schema with no members mirrors the legacy `parser == null`
    // case: the body is never parsed OR content-type checked, whatever it
    // contains (rpcv2Cbor's NoInputOutput tests send an empty CBOR map).
    // An empty body reads as a structure with no members present —
    // `@required` enforcement stays in `build()`.
    //
    // Event-stream inputs skip the Content-Type check: the HTTP header carries
    // the event-stream content type, and the "body" handed here is the
    // initial-request frame's codec payload.
    if !body.is_empty() && !schema.members().is_empty() {
        check_content_type(&parts.headers, Some(codec_content_type)).map_err(R::from)?;
        let mut deserializer = codec.create_deserializer(body);
        f(&mut deserializer).map_err(R::from)
    } else {
        f(&mut EmptyStructDeserializer).map_err(R::from)
    }
}

// ============================================================================
// restJson1
// ============================================================================

impl ServerProtocol for RestJson1 {
    type Codec = JsonCodec;
    type RequestRejection = super::rest_json_1::rejection::RequestRejection;

    fn codec() -> &'static Self::Codec {
        static CODEC: LazyLock<JsonCodec> = LazyLock::new(|| {
            JsonCodec::new(
                JsonCodecSettings::builder()
                    .use_json_name(true)
                    .default_timestamp_format(aws_smithy_types::date_time::Format::EpochSeconds)
                    // Server semantics: `@timestampFormat` is enforced, not
                    // coerced (Smithy malformed-timestamp protocol tests).
                    .build(),
            )
        });
        &CODEC
    }

    fn with_request_deserializer<R>(
        schema: &Schema<'_>,
        output_schema: &Schema<'_>,
        parts: &http::request::Parts,
        body: &[u8],
        f: impl FnOnce(&mut dyn ShapeDeserializer) -> Result<R, DeserializeError>,
    ) -> Result<R, Self::RequestRejection> {
        // Legacy generated `from_request` checks `Accept` against the
        // response content type (payload `@mediaType` aware) before
        // anything else.
        if !accept_matches_output(
            &parts.headers,
            output_schema,
            "application/json",
            <Self as EventStreamProtocol>::EVENT_STREAM_HTTP_CONTENT_TYPE,
        ) {
            return Err(Self::RequestRejection::NotAcceptable);
        }
        deserialize_rest_request(Self::codec(), "application/json", true, schema, parts, body, f)
    }

    fn serialize_response(
        schema: &Schema<'_>,
        output: &dyn SerializableStruct,
    ) -> http::Response<BoxBody> {
        if schema.original_name().is_none() {
            return no_modeled_output_response(schema, None);
        }
        let result = serialize_split(Self::codec(), schema, output, true).and_then(|split| {
            let status = resolve_status(split.status, schema);
            assemble_response(split, status, "application/json")
        });
        match result {
            Ok(response) => response,
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

    fn serialize_error(error: &dyn HttpModeledError) -> http::Response<BoxBody> {
        let schema = error.schema();
        let name = schema.shape_id().shape_name();
        // restJson1 carries no body discriminator; the error name travels in
        // the `x-amzn-errortype` header.
        let result =
            serialize_split(Self::codec(), schema, &AsSerializable(error), true).and_then(
                |split| assemble_response(split, error.status_code(), "application/json"),
            );
        match result {
            Ok(mut response) => {
                // Shape name only — the settled post-#1982 behavior. The
                // legacy hard-coded `ValidationException` header for custom
                // validation shapes was a confirmed bug (2f); the schema path
                // emits the actual shape name.
                if let Ok(value) = http::HeaderValue::try_from(name) {
                    response
                        .headers_mut()
                        .insert(http::HeaderName::from_static("x-amzn-errortype"), value);
                }
                stamp_error_extension(response, name)
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

static AMZ_JSON_10_MIME: LazyLock<mime::Mime> =
    LazyLock::new(|| "application/x-amz-json-1.0".parse().expect("valid mime"));
static AMZ_JSON_11_MIME: LazyLock<mime::Mime> =
    LazyLock::new(|| "application/x-amz-json-1.1".parse().expect("valid mime"));
static APPLICATION_CBOR_MIME: LazyLock<mime::Mime> =
    LazyLock::new(|| "application/cbor".parse().expect("valid mime"));

impl EventStreamProtocol for RestJson1 {
    const EVENT_PAYLOAD_CONTENT_TYPE: &'static str = "application/json";
    const EVENT_STREAM_HTTP_CONTENT_TYPE: &'static str = "application/vnd.amazon.eventstream";
    const FRAMES_INITIAL_MESSAGES: bool = false;
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
                // Server semantics: `@timestampFormat` is enforced, not
                // coerced (Smithy malformed-timestamp protocol tests).
                .build(),
        )
    });
    &CODEC
}

macro_rules! aws_json_impl {
    ($marker:ty, $content_type:literal, $mime:ident, $type_value:ident) => {
        impl ServerProtocol for $marker {
            type Codec = JsonCodec;
            type RequestRejection = super::aws_json::rejection::RequestRejection;

            fn codec() -> &'static Self::Codec {
                aws_json_codec()
            }

            fn with_request_deserializer<R>(
                schema: &Schema<'_>,
                _output_schema: &Schema<'_>,
                parts: &http::request::Parts,
                body: &[u8],
                f: impl FnOnce(&mut dyn ShapeDeserializer) -> Result<R, DeserializeError>,
            ) -> Result<R, Self::RequestRejection> {
                if !super::accept_header_classifier(&parts.headers, &$mime) {
                    return Err(Self::RequestRejection::NotAcceptable);
                }
                deserialize_rpc_request(Self::codec(), $content_type, schema, parts, body, f)
            }

            fn serialize_response(
                schema: &Schema<'_>,
                output: &dyn SerializableStruct,
            ) -> http::Response<BoxBody> {
                if schema.original_name().is_none() {
                    // awsJson keeps its protocol Content-Type on the empty
                    // response (AwsJson1xServiceRespondsWithNoPayload).
                    return no_modeled_output_response(schema, Some($content_type));
                }
                let result =
                    serialize_split(Self::codec(), schema, output, false).and_then(|split| {
                        let status = resolve_status(split.status, schema);
                        assemble_response(split, status, $content_type)
                    });
                match result {
                    Ok(response) => response,
                    Err(err) => {
                        log_serialize_failure!(err);
                        IntoResponse::<$marker>::into_response(
                            super::aws_json::runtime_error::RuntimeError::Serialization(
                                crate::Error::new(err),
                            ),
                        )
                    }
                }
            }

            fn serialize_error(error: &dyn HttpModeledError) -> http::Response<BoxBody> {
                let schema = error.schema();
                // `__type` written after the modeled members (legacy order).
                let wrapper = WithTypeLast {
                    type_value: $type_value(schema),
                    inner: &AsSerializable(error),
                };
                let result = serialize_split(Self::codec(), schema, &wrapper, false)
                    .and_then(|split| {
                        assemble_response(split, error.status_code(), $content_type)
                    });
                match result {
                    Ok(response) => {
                        stamp_error_extension(response, schema.shape_id().shape_name())
                    }
                    Err(err) => {
                        log_serialize_failure!(err);
                        IntoResponse::<$marker>::into_response(
                            super::aws_json::runtime_error::RuntimeError::Serialization(
                                crate::Error::new(err),
                            ),
                        )
                    }
                }
            }
        }
    };
}

/// awsJson 1.0 discriminator: the full `namespace#Name` shape ID.
fn full_shape_id<'s>(schema: &'s Schema<'s>) -> &'s str {
    schema.shape_id().as_str()
}

/// awsJson 1.1 discriminator: the shape name only.
fn shape_name_only<'s>(schema: &'s Schema<'s>) -> &'s str {
    schema.shape_id().shape_name()
}

aws_json_impl!(
    AwsJson1_0,
    "application/x-amz-json-1.0",
    AMZ_JSON_10_MIME,
    full_shape_id
);
aws_json_impl!(
    AwsJson1_1,
    "application/x-amz-json-1.1",
    AMZ_JSON_11_MIME,
    shape_name_only
);

impl EventStreamProtocol for AwsJson1_0 {
    const EVENT_PAYLOAD_CONTENT_TYPE: &'static str = "application/json";
    const EVENT_STREAM_HTTP_CONTENT_TYPE: &'static str = "application/x-amz-json-1.0";
    const FRAMES_INITIAL_MESSAGES: bool = true;
}

impl EventStreamProtocol for AwsJson1_1 {
    const EVENT_PAYLOAD_CONTENT_TYPE: &'static str = "application/json";
    const EVENT_STREAM_HTTP_CONTENT_TYPE: &'static str = "application/x-amz-json-1.1";
    const FRAMES_INITIAL_MESSAGES: bool = true;
}

// ============================================================================
// rpcv2Cbor
// ============================================================================

impl ServerProtocol for RpcV2Cbor {
    type Codec = CborCodec;
    type RequestRejection = super::rpc_v2_cbor::rejection::RequestRejection;

    fn codec() -> &'static Self::Codec {
        static CODEC: LazyLock<CborCodec> =
            LazyLock::new(|| CborCodec::new(CborCodecSettings::default()));
        &CODEC
    }

    fn with_request_deserializer<R>(
        schema: &Schema<'_>,
        _output_schema: &Schema<'_>,
        parts: &http::request::Parts,
        body: &[u8],
        f: impl FnOnce(&mut dyn ShapeDeserializer) -> Result<R, DeserializeError>,
    ) -> Result<R, Self::RequestRejection> {
        // The `smithy-protocol: rpc-v2-cbor` header is validated by the
        // router; the body content type is validated here.
        if !super::accept_header_classifier(&parts.headers, &APPLICATION_CBOR_MIME) {
            return Err(Self::RequestRejection::NotAcceptable);
        }
        deserialize_rpc_request(Self::codec(), "application/cbor", schema, parts, body, f)
    }

    fn serialize_response(
        schema: &Schema<'_>,
        output: &dyn SerializableStruct,
    ) -> http::Response<BoxBody> {
        if schema.original_name().is_none() {
            // rpcv2Cbor forbids Content-Type on the empty response
            // (RpcV2CborNoOutput) but keeps `smithy-protocol`.
            let mut response = no_modeled_output_response(schema, None);
            response.headers_mut().insert(
                http::HeaderName::from_static("smithy-protocol"),
                http::HeaderValue::from_static("rpc-v2-cbor"),
            );
            return response;
        }
        let result = serialize_split(Self::codec(), schema, output, false).and_then(|split| {
            let status = resolve_status(split.status, schema);
            assemble_response(split, status, "application/cbor")
        });
        match result {
            Ok(mut response) => {
                response.headers_mut().insert(
                    http::HeaderName::from_static("smithy-protocol"),
                    http::HeaderValue::from_static("rpc-v2-cbor"),
                );
                response
            }
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

    fn serialize_error(error: &dyn HttpModeledError) -> http::Response<BoxBody> {
        let schema = error.schema();
        // Full shape ID as the FIRST map entry (legacy
        // `AddTypeFieldToServerErrorsCborCustomization` order).
        let wrapper = WithTypeFirst {
            type_value: schema.shape_id().as_str(),
            inner: &AsSerializable(error),
        };
        let result = serialize_split(Self::codec(), schema, &wrapper, false)
            .and_then(|split| assemble_response(split, error.status_code(), "application/cbor"));
        match result {
            Ok(mut response) => {
                response.headers_mut().insert(
                    http::HeaderName::from_static("smithy-protocol"),
                    http::HeaderValue::from_static("rpc-v2-cbor"),
                );
                stamp_error_extension(response, schema.shape_id().shape_name())
            }
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

impl EventStreamProtocol for RpcV2Cbor {
    const EVENT_PAYLOAD_CONTENT_TYPE: &'static str = "application/cbor";
    const EVENT_STREAM_HTTP_CONTENT_TYPE: &'static str = "application/vnd.amazon.eventstream";
    const FRAMES_INITIAL_MESSAGES: bool = true;
}

// ============================================================================
// restXml
// ============================================================================

impl ServerProtocol for RestXml {
    type Codec = XmlCodec;
    type RequestRejection = super::rest_xml::rejection::RequestRejection;

    fn codec() -> &'static Self::Codec {
        static CODEC: LazyLock<XmlCodec> =
            LazyLock::new(|| XmlCodec::new(XmlCodecSettings::default()));
        &CODEC
    }

    fn with_request_deserializer<R>(
        schema: &Schema<'_>,
        output_schema: &Schema<'_>,
        parts: &http::request::Parts,
        body: &[u8],
        f: impl FnOnce(&mut dyn ShapeDeserializer) -> Result<R, DeserializeError>,
    ) -> Result<R, Self::RequestRejection> {
        if !accept_matches_output(
            &parts.headers,
            output_schema,
            "application/xml",
            <Self as EventStreamProtocol>::EVENT_STREAM_HTTP_CONTENT_TYPE,
        ) {
            return Err(Self::RequestRejection::NotAcceptable);
        }
        deserialize_rest_request(Self::codec(), "application/xml", true, schema, parts, body, f)
    }

    fn serialize_response(
        schema: &Schema<'_>,
        output: &dyn SerializableStruct,
    ) -> http::Response<BoxBody> {
        if schema.original_name().is_none() {
            return no_modeled_output_response(schema, None);
        }
        let result = serialize_split(Self::codec(), schema, output, true).and_then(|split| {
            let status = resolve_status(split.status, schema);
            assemble_response(split, status, "application/xml")
        });
        match result {
            Ok(response) => response,
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

    fn serialize_error(error: &dyn HttpModeledError) -> http::Response<BoxBody> {
        // Known divergence, deliberate (2f, fix-forward): today's generated
        // restXml server error bodies are broken (bare `<Error>` envelope no
        // client parses, and the runtime discards pre-rendered
        // validation/framework bodies in favor of a literal `"{}"`).
        // Freezing that behavior would freeze a bug, so the schema path
        // serializes the error structure through the XML codec as-is. See
        // assumptions register B4/B6; gated by its own pinned goldens.
        let schema = error.schema();
        let result = serialize_split(Self::codec(), schema, &AsSerializable(error), true)
            .and_then(|split| assemble_response(split, error.status_code(), "application/xml"));
        match result {
            Ok(response) => stamp_error_extension(response, schema.shape_id().shape_name()),
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

impl EventStreamProtocol for RestXml {
    const EVENT_PAYLOAD_CONTENT_TYPE: &'static str = "application/xml";
    const EVENT_STREAM_HTTP_CONTENT_TYPE: &'static str = "application/vnd.amazon.eventstream";
    const FRAMES_INITIAL_MESSAGES: bool = false;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modeled_error::ModeledError;
    use crate::protocol::test_helpers::get_body_as_string;
    use aws_smithy_schema::serde::ShapeDeserializer;
    use aws_smithy_schema::traits::HttpTrait;

    // ------------------------------------------------------------------
    // A hand-built operation input: label + query + header + body member.
    // ------------------------------------------------------------------

    static NAME_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#In$name", "test", "In"),
        ShapeType::String,
        "name",
        0,
    )
    .with_http_label();
    static AGE_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#In$age", "test", "In"),
        ShapeType::Integer,
        "age",
        1,
    )
    .with_http_query("age");
    static NOTE_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#In$note", "test", "In"),
        ShapeType::String,
        "note",
        2,
    );
    static IN_MEMBERS: [&Schema<'static>; 3] = [&NAME_MEMBER, &AGE_MEMBER, &NOTE_MEMBER];
    static IN_SCHEMA: Schema<'static> = Schema::new_struct(
        ShapeId::from_parts("test#In", "test", "In"),
        ShapeType::Structure,
        &IN_MEMBERS,
    )
    .with_http(HttpTrait::new("POST", "/pets/{name}", Some(200)));

    // Body-only variant for the RPC protocols (no @http bindings at all).
    static RPC_NOTE_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#RpcIn$note", "test", "RpcIn"),
        ShapeType::String,
        "note",
        0,
    );
    static RPC_IN_MEMBERS: [&Schema<'static>; 1] = [&RPC_NOTE_MEMBER];
    static RPC_IN_SCHEMA: Schema<'static> = Schema::new_struct(
        ShapeId::from_parts("test#RpcIn", "test", "RpcIn"),
        ShapeType::Structure,
        &RPC_IN_MEMBERS,
    )
    .with_original_name("RpcIn");

    #[derive(Debug, Default, PartialEq)]
    struct TestInput {
        name: Option<String>,
        age: Option<i32>,
        note: Option<String>,
    }

    impl TestInput {
        fn walk(
            schema: &'static Schema<'static>,
            deserializer: &mut dyn ShapeDeserializer,
        ) -> Result<Self, DeserializeError> {
            let mut out = TestInput::default();
            deserializer.read_struct(schema, &mut |member, d| {
                match member.member_name() {
                    Some("name") => out.name = Some(d.read_string(member)?),
                    Some("age") => out.age = Some(d.read_integer(member)?),
                    Some("note") => out.note = Some(d.read_string(member)?),
                    _ => {}
                }
                Ok(())
            })?;
            Ok(out)
        }
    }

    impl DeserializableShape for TestInput {
        fn deserialize(
            deserializer: &mut dyn ShapeDeserializer,
        ) -> Result<Self, DeserializeError> {
            Self::walk(&IN_SCHEMA, deserializer)
        }
    }

    /// The same walker against the body-only RPC schema.
    #[derive(Debug, Default, PartialEq)]
    struct RpcTestInput(TestInput);

    impl DeserializableShape for RpcTestInput {
        fn deserialize(
            deserializer: &mut dyn ShapeDeserializer,
        ) -> Result<Self, DeserializeError> {
            TestInput::walk(&RPC_IN_SCHEMA, deserializer).map(RpcTestInput)
        }
    }

    fn parts(uri: &str, headers: &[(&'static str, &str)]) -> http::request::Parts {
        let mut builder = http::Request::builder().method("POST").uri(uri);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        builder.body(()).unwrap().into_parts().0
    }

    static EMPTY_IN_MEMBERS: [&Schema<'static>; 0] = [];
    static EMPTY_IN_SCHEMA: Schema<'static> = Schema::new_struct(
        ShapeId::from_parts("test#EmptyIn", "test", "EmptyIn"),
        ShapeType::Structure,
        &EMPTY_IN_MEMBERS,
    )
    .with_http(HttpTrait::new("POST", "/empty", Some(200)));

    #[derive(Debug)]
    struct EmptyInput;
    impl DeserializableShape for EmptyInput {
        fn deserialize(
            deserializer: &mut dyn ShapeDeserializer,
        ) -> Result<Self, DeserializeError> {
            deserializer.read_struct(&EMPTY_IN_SCHEMA, &mut |_, _| Ok(()))?;
            Ok(EmptyInput)
        }
    }

    #[test]
    fn request_paths() {
        use super::super::rest_json_1::rejection::RequestRejection;

        // REST: label + query + body member route through the composite.
        let p = parts("/pets/rex?age=7", &[("content-type", "application/json")]);
        let input: TestInput =
            RestJson1::deserialize_request(&IN_SCHEMA, &IN_SCHEMA, &p, br#"{"note":"hi"}"#).unwrap();
        assert_eq!(
            input,
            TestInput {
                name: Some("rex".to_string()),
                age: Some(7),
                note: Some("hi".to_string()),
            }
        );

        // Wrong content type (non-empty body) and bad Accept are rejected.
        let p = parts("/pets/rex", &[("content-type", "text/xml")]);
        assert!(matches!(
            RestJson1::deserialize_request::<TestInput>(&IN_SCHEMA, &IN_SCHEMA, &p, b"{}").unwrap_err(),
            RequestRejection::MissingContentType(_)
        ));
        let p = parts(
            "/pets/rex",
            &[("content-type", "application/json"), ("accept", "text/xml")],
        );
        assert!(matches!(
            RestJson1::deserialize_request::<TestInput>(&IN_SCHEMA, &IN_SCHEMA, &p, b"{}").unwrap_err(),
            RequestRejection::NotAcceptable
        ));

        // The legacy `if !bytes.is_empty()` gate: no content type required
        // when no body was sent, even though body members are modeled.
        let p = parts("/pets/rex", &[]);
        let input: TestInput = RestJson1::deserialize_request(&IN_SCHEMA, &IN_SCHEMA, &p, b"").unwrap();
        assert_eq!(input.name.as_deref(), Some("rex"));
        assert_eq!(input.note, None);

        // `serverContentTypeCheckNoModeledInput`: content-type must NOT be
        // present when the operation has no modeled input.
        let p = parts("/empty", &[("content-type", "application/json")]);
        assert!(matches!(
            RestJson1::deserialize_request::<EmptyInput>(&EMPTY_IN_SCHEMA, &EMPTY_IN_SCHEMA, &p, b"")
                .unwrap_err(),
            RequestRejection::MissingContentType(_)
        ));
        let p = parts("/empty", &[]);
        RestJson1::deserialize_request::<EmptyInput>(&EMPTY_IN_SCHEMA, &EMPTY_IN_SCHEMA, &p, b"").unwrap();

        // RPC: body round-trips through the protocol's own codec.
        use aws_smithy_schema::codec::FinishSerializer;
        struct Body;
        impl SerializableStruct for Body {
            fn serialize_members(
                &self,
                s: &mut dyn ShapeSerializer,
            ) -> Result<(), SerdeError> {
                s.write_string(&RPC_NOTE_MEMBER, "hi")
            }
        }
        let mut serializer = <RpcV2Cbor as ServerProtocol>::codec().create_serializer();
        serializer.write_struct(&RPC_IN_SCHEMA, &Body).unwrap();
        let body = serializer.finish();
        let p = parts("/service/Op", &[("content-type", "application/cbor")]);
        let input: RpcTestInput =
            RpcV2Cbor::deserialize_request(&RPC_IN_SCHEMA, &RPC_IN_SCHEMA, &p, &body).unwrap();
        assert_eq!(input.0.note.as_deref(), Some("hi"));

        // RPC empty body: members stay unset (`build()` owns @required).
        let p = parts(
            "/service/Op",
            &[("content-type", "application/x-amz-json-1.0")],
        );
        let input: RpcTestInput =
            AwsJson1_0::deserialize_request(&RPC_IN_SCHEMA, &RPC_IN_SCHEMA, &p, b"").unwrap();
        assert_eq!(input.0, TestInput::default());
    }

    // ------------------------------------------------------------------
    // Responses
    // ------------------------------------------------------------------

    static OUT_MSG_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#Out$msg", "test", "Out"),
        ShapeType::String,
        "msg",
        0,
    );
    static OUT_MEMBERS: [&Schema<'static>; 1] = [&OUT_MSG_MEMBER];
    static OUT_SCHEMA: Schema<'static> = Schema::new_struct(
        ShapeId::from_parts("test#Out", "test", "Out"),
        ShapeType::Structure,
        &OUT_MEMBERS,
    )
    .with_http(HttpTrait::new("POST", "/pets/{name}", Some(201)))
    .with_original_name("Out");

    struct TestOutput;
    impl SerializableStruct for TestOutput {
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            s.write_string(&OUT_MSG_MEMBER, "ok")
        }
    }

    #[tokio::test]
    async fn response_paths() {
        // REST: status from the @http trait, protocol content type, codec body.
        let response = RestJson1::serialize_response(&OUT_SCHEMA, &TestOutput);
        assert_eq!(response.status(), http::StatusCode::CREATED);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/json"
        );
        let body = get_body_as_string(response.into_body()).await;
        assert_eq!(body, r#"{"msg":"ok"}"#);

        // RPC: default status, protocol headers stamped.
        let response = RpcV2Cbor::serialize_response(&RPC_IN_SCHEMA, &TestOutput);
        assert_eq!(response.status(), http::StatusCode::OK);
        assert_eq!(
            response.headers().get("smithy-protocol").unwrap(),
            "rpc-v2-cbor"
        );
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/cbor"
        );
    }

    // ------------------------------------------------------------------
    // Errors and the 2d validation seam
    // ------------------------------------------------------------------

    static BOOM_MSG_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#Boom$message", "test", "Boom"),
        ShapeType::String,
        "message",
        0,
    );
    static BOOM_HDR_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#Boom$tag", "test", "Boom"),
        ShapeType::String,
        "tag",
        1,
    )
    .with_http_header("x-boom-tag");
    static BOOM_MEMBERS: [&Schema<'static>; 2] = [&BOOM_MSG_MEMBER, &BOOM_HDR_MEMBER];
    static BOOM_SCHEMA: Schema<'static> = Schema::new_struct(
        ShapeId::from_parts("test#Boom", "test", "Boom"),
        ShapeType::Structure,
        &BOOM_MEMBERS,
    );

    #[derive(Debug)]
    struct Boom;

    impl std::fmt::Display for Boom {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("boom happened")
        }
    }

    impl SerializableStruct for Boom {
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            s.write_string(&BOOM_MSG_MEMBER, "boom happened")?;
            s.write_string(&BOOM_HDR_MEMBER, "tagged")
        }
    }

    impl ModeledError for Boom {
        fn schema(&self) -> &Schema<'_> {
            &BOOM_SCHEMA
        }
    }

    impl HttpModeledError for Boom {
        fn status_code(&self) -> u16 {
            422
        }
    }

    #[tokio::test]
    async fn error_framing_and_validation_seam() {
        // restJson1: name-only header discriminator, status from
        // status_code(), @httpHeader-bound error member split out of the body.
        let response = RestJson1::serialize_error(&Boom);
        assert_eq!(response.status(), http::StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(response.headers().get("x-amzn-errortype").unwrap(), "Boom");
        assert_eq!(response.headers().get("x-boom-tag").unwrap(), "tagged");
        let body = get_body_as_string(response.into_body()).await;
        assert_eq!(body, r#"{"message":"boom happened"}"#);

        // awsJson 1.0: full shape ID written last; header-bound members are
        // NOT split on RPC protocols. awsJson 1.1: name only.
        let body = get_body_as_string(AwsJson1_0::serialize_error(&Boom).into_body()).await;
        assert!(body.contains(r#""tag":"tagged""#), "{body}");
        assert!(body.ends_with(r#""__type":"test#Boom"}"#), "{body}");
        let body = get_body_as_string(AwsJson1_1::serialize_error(&Boom).into_body()).await;
        assert!(body.ends_with(r#""__type":"Boom"}"#), "{body}");

        // rpcv2Cbor: full shape ID as the FIRST map entry.
        let response = RpcV2Cbor::serialize_error(&Boom);
        assert_eq!(
            response.headers().get("smithy-protocol").unwrap(),
            "rpc-v2-cbor"
        );
        use http_body_util::BodyExt;
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body collects")
            .to_bytes();
        let type_pos = bytes
            .windows(6)
            .position(|w| w == b"__type")
            .expect("__type present");
        let msg_pos = bytes
            .windows(7)
            .position(|w| w == b"message")
            .expect("message present");
        assert!(type_pos < msg_pos, "__type must be the first map entry");
        assert!(
            bytes.windows(9).any(|w| w == b"test#Boom"),
            "full shape ID present"
        );

        // The 2d seam: walker constraint-violation channel → rejection →
        // RuntimeError::Validation → serialized exactly ONCE, by the
        // protocol, at the boundary, with the ACTUAL shape name (2f
        // fix-forward — not the legacy hard-coded `ValidationException`).
        use super::super::rest_json_1::rejection::RequestRejection;
        use super::super::rest_json_1::runtime_error::RuntimeError;
        use crate::response::IntoResponse;
        let rejection: RequestRejection =
            DeserializeError::ConstraintViolation(Box::new(Boom)).into();
        let runtime_error = RuntimeError::from(rejection);
        assert!(matches!(runtime_error, RuntimeError::Validation(_)));
        let response = IntoResponse::<RestJson1>::into_response(runtime_error);
        assert_eq!(response.status(), http::StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(response.headers().get("x-amzn-errortype").unwrap(), "Boom");
        let body = get_body_as_string(response.into_body()).await;
        assert_eq!(body, r#"{"message":"boom happened"}"#);
    }
}
