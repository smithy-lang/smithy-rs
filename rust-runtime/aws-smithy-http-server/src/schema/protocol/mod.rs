/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The server-side protocol trait: schema-driven request deserialization and
//! output/error serialization (plan 2a).
//!
//! [`ServerProtocol`] is the erased runtime protocol object used after routing
//! has selected a protocol and operation. It is responsible for protocol
//! serialization and deserialization only; route claiming lives in
//! `routing::ProtocolRouter`.
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

pub(crate) mod aws_json;
mod discriminator;
mod request;
mod response;
pub(crate) mod rest_json;
pub(crate) mod rest_xml;
pub(crate) mod rpc_v2_cbor;
#[cfg(test)]
mod tests;

use aws_smithy_schema::codec::{Codec, DynCodec};
use aws_smithy_schema::serde::{SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{Schema, ShapeId};
use bytes::Bytes;
use std::{any::Any, fmt, future::Future, pin::Pin, sync::Arc};

use crate::body::{Body, BoxBody};
use crate::deserialize::{DeserializableShape, DeserializeError};
use crate::modeled_error::{HttpModeledError, HttpServerError, ServerError};
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
    /// Event-stream operation glue uses this seam to walk into the input's
    /// internal BUILDER, attach the frame receiver, and only then `build()`
    /// (the stream member is `@required`-equivalent).
    fn with_request_deserializer<R>(
        schema: &Schema<'_>,
        parts: &http::request::Parts,
        body: &[u8],
        f: impl FnOnce(&mut dyn ShapeDeserializer) -> Result<R, DeserializeError>,
    ) -> Result<R, Self::RequestRejection>;

    /// Creates a request deserializer over the collected HTTP request.
    fn request_deserializer<'a>(
        schema: &Schema<'_>,
        request: &'a http::Request<Bytes>,
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

/// Type-erased generated input deserializer used by dynamic server protocols.
///
/// Dynamic protocols own HTTP body handling. Once a protocol has built the
/// correct shape deserializer for its wire format, this object drives the
/// generated input walker and returns the concrete operation input erased as
/// `Any`.
pub trait DynInputDeserializer: Send {
    /// Deserializes an operation input from the protocol-owned shape deserializer.
    fn deserialize(
        self: Box<Self>,
        deserializer: &mut dyn ShapeDeserializer,
    ) -> Result<Box<dyn Any + Send>, DeserializeError>;
}

/// A concrete dynamic input visitor for `T`.
pub struct DynInputVisitor<T>(std::marker::PhantomData<T>);

impl<T> DynInputVisitor<T> {
    /// Creates a new dynamic input visitor.
    pub fn new() -> Self {
        Self(std::marker::PhantomData)
    }
}

impl<T> DynInputDeserializer for DynInputVisitor<T>
where
    T: DeserializableShape + Send + 'static,
{
    fn deserialize(
        self: Box<Self>,
        deserializer: &mut dyn ShapeDeserializer,
    ) -> Result<Box<dyn Any + Send>, DeserializeError> {
        Ok(Box::new(T::deserialize(deserializer)?))
    }
}

/// Future returned by dynamic request deserialization.
pub type DeserializeInputFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Box<dyn Any + Send>, ServerError>> + Send + 'a>>;

/// Static authoring trait for server protocols.
///
/// This is the server-side analogue of the client's
/// `ClientProtocolInner`, except the request type is a trait parameter so
/// service stacks can choose their body type. Implementors write this trait;
/// the object-safe [`ServerProtocol`] view is provided by a blanket impl.
pub trait ServerProtocolInner<Req = http::Request<Bytes>>: Send + Sync + fmt::Debug {
    /// Returns the Smithy protocol shape ID.
    fn protocol_id(&self) -> &ShapeId<'static>;

    /// Returns this protocol's payload/body codec.
    fn codec(&self) -> &dyn DynCodec;

    /// Creates a request deserializer for the selected protocol.
    fn deserialize_request<'a>(
        &self,
        request: &'a Req,
        input_schema: &Schema<'_>,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, ServerError>;

    /// Deserializes a dynamic operation input from the original HTTP request.
    ///
    /// Implementations own body collection, content-type validation, and
    /// event-stream body handling. The default implementation is intentionally
    /// absent because returning borrowed deserializers from collected bytes is
    /// not sound; concrete protocols must drive `input` inside this future.
    fn deserialize_input<'a>(
        &'a self,
        request: http::Request<Body>,
        input_schema: &'static Schema<'static>,
        request_body_max_bytes: usize,
        input: Box<dyn DynInputDeserializer>,
    ) -> DeserializeInputFuture<'a>;

    /// Serializes a successful operation output.
    fn serialize_response(&self, schema: &Schema<'_>, output: &dyn SerializableStruct) -> http::Response<BoxBody>;

    /// Serializes an operation or framework server error.
    fn serialize_error(&self, error: &dyn HttpServerError) -> http::Response<BoxBody>;

    /// Frame-level `:content-type` for event payloads, when supported.
    fn event_payload_content_type(&self) -> Option<&'static str> {
        None
    }

    /// HTTP-level `Content-Type` for event streams, when supported.
    fn event_stream_http_content_type(&self) -> Option<&'static str> {
        None
    }

    /// Whether this protocol frames RPC initial event-stream messages.
    fn frames_initial_messages(&self) -> bool {
        false
    }
}

/// Object-safe server protocol view used by schema-driven dynamic dispatch.
///
/// This mirrors the client-side `ClientProtocol` split: concrete protocols may
/// keep static [`ServerProtocolInner`] implementations, while dynamic routing
/// and upgrade code hold a shared erased protocol object.
pub trait ServerProtocol<Req = http::Request<Bytes>>: Send + Sync + fmt::Debug {
    /// Returns the Smithy protocol shape ID.
    fn protocol_id(&self) -> &ShapeId<'static>;

    /// Returns this protocol's payload/body codec.
    fn codec(&self) -> &dyn DynCodec;

    /// Creates a request deserializer for the selected protocol.
    fn deserialize_request<'a>(
        &self,
        request: &'a Req,
        input_schema: &Schema<'_>,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, ServerError>;

    /// Deserializes a dynamic operation input from the original HTTP request.
    fn deserialize_input<'a>(
        &'a self,
        request: http::Request<Body>,
        input_schema: &'static Schema<'static>,
        request_body_max_bytes: usize,
        input: Box<dyn DynInputDeserializer>,
    ) -> DeserializeInputFuture<'a>;

    /// Serializes a successful operation output.
    fn serialize_response(&self, schema: &Schema<'_>, output: &dyn SerializableStruct) -> http::Response<BoxBody>;

    /// Serializes an operation or framework server error.
    fn serialize_error(&self, error: &dyn HttpServerError) -> http::Response<BoxBody>;

    /// Frame-level `:content-type` for event payloads, when supported.
    fn event_payload_content_type(&self) -> Option<&'static str> {
        None
    }

    /// HTTP-level `Content-Type` for event streams, when supported.
    fn event_stream_http_content_type(&self) -> Option<&'static str> {
        None
    }

    /// Whether this protocol frames RPC initial event-stream messages.
    fn frames_initial_messages(&self) -> bool {
        false
    }
}

impl<P, Req> ServerProtocol<Req> for P
where
    P: ServerProtocolInner<Req>,
{
    fn protocol_id(&self) -> &ShapeId<'static> {
        <Self as ServerProtocolInner<Req>>::protocol_id(self)
    }

    fn codec(&self) -> &dyn DynCodec {
        <Self as ServerProtocolInner<Req>>::codec(self)
    }

    fn deserialize_request<'a>(
        &self,
        request: &'a Req,
        input_schema: &Schema<'_>,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, ServerError> {
        <Self as ServerProtocolInner<Req>>::deserialize_request(self, request, input_schema)
    }

    fn deserialize_input<'a>(
        &'a self,
        request: http::Request<Body>,
        input_schema: &'static Schema<'static>,
        request_body_max_bytes: usize,
        input: Box<dyn DynInputDeserializer>,
    ) -> DeserializeInputFuture<'a> {
        <Self as ServerProtocolInner<Req>>::deserialize_input(
            self,
            request,
            input_schema,
            request_body_max_bytes,
            input,
        )
    }

    fn serialize_response(&self, schema: &Schema<'_>, output: &dyn SerializableStruct) -> http::Response<BoxBody> {
        <Self as ServerProtocolInner<Req>>::serialize_response(self, schema, output)
    }

    fn serialize_error(&self, error: &dyn HttpServerError) -> http::Response<BoxBody> {
        <Self as ServerProtocolInner<Req>>::serialize_error(self, error)
    }

    fn event_payload_content_type(&self) -> Option<&'static str> {
        <Self as ServerProtocolInner<Req>>::event_payload_content_type(self)
    }

    fn event_stream_http_content_type(&self) -> Option<&'static str> {
        <Self as ServerProtocolInner<Req>>::event_stream_http_content_type(self)
    }

    fn frames_initial_messages(&self) -> bool {
        <Self as ServerProtocolInner<Req>>::frames_initial_messages(self)
    }
}

/// Shared erased server protocol.
pub struct SharedServerProtocol<Req = http::Request<Bytes>> {
    inner: Arc<dyn ServerProtocol<Req>>,
}

impl<Req> Clone for SharedServerProtocol<Req> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<Req> SharedServerProtocol<Req> {
    /// Creates a shared erased protocol from a concrete erased protocol value.
    pub fn new(protocol: impl ServerProtocol<Req> + 'static) -> Self {
        Self {
            inner: Arc::new(protocol),
        }
    }
}

impl<Req> std::fmt::Debug for SharedServerProtocol<Req> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedServerProtocol")
            .field("protocol_id", &self.protocol_id())
            .finish()
    }
}

impl<Req> std::ops::Deref for SharedServerProtocol<Req> {
    type Target = dyn ServerProtocol<Req>;

    fn deref(&self) -> &Self::Target {
        &*self.inner
    }
}
