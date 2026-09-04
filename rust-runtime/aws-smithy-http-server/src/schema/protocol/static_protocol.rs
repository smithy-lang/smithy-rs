/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_schema::codec::{Codec, DynCodec};
use aws_smithy_schema::serde::{SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::Schema;

use crate::body::BoxBody;
use crate::deserialize::{DeserializableShape, DeserializeError};
use crate::modeled_error::HttpModeledError;
use crate::protocol::ProtocolShape;

pub trait StaticProtocol: ProtocolShape + 'static {
    /// Body codec. Also the event-stream frame-payload codec — the client
    /// needs a dyn `payload_codec()` accessor because its protocol is a
    /// runtime value; server dispatch is static, so `Self::Codec` serves
    /// both.
    ///
    /// Associated type, not `DynCodec`: `FinishSerializer::finish` is not
    /// object-safe, and no protocol-erased call site exists server-side.
    type Codec: Codec + DynCodec + 'static;

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
        parts: &http::request::Parts,
        body: &[u8],
    ) -> Result<T, Self::RequestRejection> {
        Self::with_request_deserializer(schema, parts, body, |deserializer| T::deserialize(deserializer))
    }

    /// Like [`deserialize_request`](Self::deserialize_request), but hands the
    /// composite deserializer to `f` instead of driving `T::deserialize`.
    /// Event-stream operation glue uses this path to walk into the input's
    /// internal builder, attach the frame receiver, and only then `build()`.
    fn with_request_deserializer<R>(
        schema: &Schema<'_>,
        parts: &http::request::Parts,
        body: &[u8],
        f: impl FnOnce(&mut dyn ShapeDeserializer) -> Result<R, DeserializeError>,
    ) -> Result<R, Self::RequestRejection>;

    /// Creates a request deserializer over the collected HTTP request.
    fn request_deserializer<'a>(
        schema: &Schema<'_>,
        request: &'a http::Request<bytes::Bytes>,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, Self::RequestRejection>;

    /// Converts a request rejection into this protocol's legacy runtime error
    /// response shape.
    fn request_rejection_into_response(rejection: Self::RequestRejection) -> http::Response<BoxBody>;

    /// Success path. Status: `@httpResponseCode` member if bound and set,
    /// else `schema.http().code()`, else `200`. REST protocols honor
    /// response bindings read off member schemas; RPC protocols serialize
    /// body-only.
    ///
    /// Serialization failure logs via `tracing` and falls back to the
    /// protocol's `RuntimeError::Serialization` response, preserving the
    /// legacy generated `IntoResponse` contract.
    fn serialize_response(schema: &Schema<'_>, output: &dyn SerializableStruct) -> http::Response<BoxBody>;

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
/// and event-stream operation impls bound on `P: ServerEventStreamProtocol`;
/// ordinary operations stay `P: ServerProtocol`. Bounds never reach
/// user-facing signatures (concrete-marker instantiation).
pub trait StaticEventStreamProtocol: StaticProtocol {
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
