/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! [`ServerProtocol`] and its implementations on the protocol markers.
//!
//! Each protocol frames modeled errors with its discriminator:
//!
//! | Protocol   | Discriminator                                            |
//! |------------|----------------------------------------------------------|
//! | restJson1  | `x-amzn-errortype` header, shape name only; none in body |
//! | awsJson1.0 | `__type` body member, full `namespace#Name`, written last |
//! | awsJson1.1 | `__type` body member, shape name only, written last      |
//! | rpcv2Cbor  | `__type` body member, full `namespace#Name`, written first |
//! | restXml    | none                                                     |
//!
//! `@httpHeader`-bound error members are split out of the body on the REST protocols.

mod aws_json;
mod discriminator;
mod request;
pub(crate) mod response;
pub(crate) mod rest;
pub(crate) mod rest_json_1;
pub(crate) mod rest_xml;
pub(crate) mod rpc;
mod rpc_v2_cbor;
pub(crate) mod rpc_v2_cbor_serde;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::time::Duration;

use aws_smithy_runtime_api::http::{Headers, Uri};
use aws_smithy_schema::codec::DynCodec;
use aws_smithy_schema::serde::{SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{Schema, ShapeId};
use bytes::Bytes;

use crate::body::{collect_body_limited, BoxBody, CollectBodyError, HttpBody};
use crate::response::Response;

use super::{DeserializeError, HttpModeledError};

/// The canonical, transport-independent view of a collected request on the schema path.
///
/// Transports convert into this at the edge, the way `LambdaHandler` (behind the `aws-lambda`
/// feature) converts a Lambda event into an HTTP request today: an HTTP server runs
/// [`Request::try_from`](aws_smithy_runtime_api::http::Request), collects the body, and builds one
/// of these; a future transport synthesizes the same fields from its own messages. The fields are
/// public precisely so such upgrade layers can construct it.
///
/// A streaming request (event stream or streaming blob input) reaches
/// [`ServerProtocol::deserialize_request`] with an empty `body`: the protocol reads the URI and
/// header bindings, and the generated streaming glue attaches the live body afterwards.
#[derive(Debug)]
pub struct ServerRequest {
    /// The request URI.
    pub uri: Uri,
    /// The request headers. Values are valid UTF-8 by construction.
    pub headers: Headers,
    /// The collected request body. Empty when the protocol answered `false` from
    /// [`ServerProtocol::reads_request_body`] or when the input is streaming.
    pub body: Bytes,
}

/// Shared, erased server protocol selected by routing.
#[derive(Clone, Debug)]
pub struct SharedServerProtocol(std::sync::Arc<dyn ServerProtocol>);

impl SharedServerProtocol {
    /// Wrap a concrete server protocol.
    pub fn new(protocol: impl ServerProtocol) -> Self {
        Self(std::sync::Arc::new(protocol))
    }
}

impl std::ops::Deref for SharedServerProtocol {
    type Target = dyn ServerProtocol;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

/// The event-frame capability of a server protocol.
pub trait ServerEventStreamProtocol: Send + Sync + std::fmt::Debug {
    /// Codec for structured event and initial-message payloads.
    fn payload_codec(&self) -> &dyn DynCodec;
    /// Media type of structured event payloads.
    fn event_stream_media_type(&self) -> &str;
    /// Whether non-stream members travel in initial-message frames.
    fn initial_messages_in_frames(&self) -> bool;
}

/// Schema-driven serialization for one protocol, keyed on struct schemas.
///
/// The request side takes the operation's input schema, the response side its output schema,
/// mirroring the client's `ClientProtocolInner` with the two directions swapped. The trait is
/// object-safe: routing stores a [`SharedServerProtocol`] in the request extensions and
/// everything after routing works through that erased handle, so nothing downstream names a
/// concrete protocol.
///
/// Nothing is precomputed per operation. Bindings, media types, URI templates and status codes are
/// derived from the schema on every call, so a struct serialized from middleware with a
/// hand-written schema is framed exactly like an operation output with the same schema.
///
/// `Debug` is a supertrait, as on the client's protocol trait, so generated marshallers holding
/// the handle can derive it.
///
/// # Serializing from middleware
///
/// After routing, an HTTP plugin can read the selected protocol from the request extensions and
/// serialize any [`SerializableStruct`] whose schema it holds:
///
/// ```no_run
/// use aws_smithy_http_server::schema::SelectedProtocolOperation;
/// use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
/// use aws_smithy_schema::{shape_id, Schema, ShapeType};
///
/// static MESSAGE: Schema<'static> =
///     Schema::new_member(shape_id!("example", "Teapot", "message"), ShapeType::String, "message", 0);
/// static MEMBERS: [&Schema<'static>; 1] = [&MESSAGE];
/// static TEAPOT: Schema<'static> =
///     Schema::new_struct(shape_id!("example", "Teapot"), ShapeType::Structure, &MEMBERS)
///         .with_original_name("Teapot");
///
/// struct Teapot;
///
/// impl SerializableStruct for Teapot {
///     fn schema(&self) -> &Schema<'_> { &TEAPOT }
///
///     fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
///         serializer.write_string(&MESSAGE, "short and stout")
///     }
/// }
///
/// fn short_circuit(request: &http::Request<()>) -> Option<http::Response<aws_smithy_http_server::body::BoxBody>> {
///     let selected = request.extensions().get::<SelectedProtocolOperation>()?;
///     let mut response = selected.protocol().serialize_response(&TEAPOT, &Teapot);
///     *response.status_mut() = http::StatusCode::IM_A_TEAPOT;
///     Some(response)
/// }
/// ```
pub trait ServerProtocol: Send + Sync + std::fmt::Debug + 'static {
    /// The protocol trait's shape ID, such as `aws.protocols#restJson1`.
    fn protocol_id(&self) -> &'static ShapeId<'static>;

    /// Event-frame support, when this protocol supports event streams.
    fn event_stream(&self) -> Option<&dyn ServerEventStreamProtocol> {
        None
    }

    /// The `Accept` gate, keyed on the output the response will carry.
    ///
    /// Runs before [`Self::deserialize_request`], so a protocol that keeps `406` and `415`
    /// distinct answers `406` first. Provided: no policy.
    fn check_accept(&self, _output: &Schema<'_>, _headers: &Headers) -> Result<(), DeserializeError> {
        Ok(())
    }

    /// Whether the collected body is needed to read `input`.
    ///
    /// Defaults to `true`. A protocol that never reads the body for some inputs may answer `false`
    /// to skip collection; [`ServerRequest::body`] is then empty. Streaming inputs are never
    /// collected, whatever this method answers.
    fn reads_request_body(&self, _input: &Schema<'_>) -> bool {
        true
    }

    /// Presents `request` as a deserializer for `input`.
    ///
    /// The `Content-Type` check happens here; the returned deserializer resolves `@http` bindings
    /// from the request and hands body members to its internal codec. Synchronous over an
    /// already collected body.
    fn deserialize_request<'a>(
        &'a self,
        input: &Schema<'_>,
        request: &'a ServerRequest,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, DeserializeError>;

    /// Serializes `value` as a complete in-memory response for `output`.
    ///
    /// The status is the `@httpResponseCode` member when bound and set, else the output schema's
    /// `@http` code, else `200`; a caller wanting another status mutates the returned response. A
    /// serialization failure is logged and answered with the protocol's `RuntimeError::Serialization`
    /// response, so this never fails outward.
    fn serialize_response(&self, output: &Schema<'_>, value: &dyn SerializableStruct) -> Response;

    /// Serializes the head of a streaming response for `output` around a caller-supplied `body`.
    ///
    /// Status, `@httpHeader`-bound members and the content type come from `output` and `value`;
    /// no body member is written. The generated streaming glue builds `body` from the event
    /// stream or streaming blob member.
    fn serialize_streaming_response(
        &self,
        output: &Schema<'_>,
        value: &dyn SerializableStruct,
        body: BoxBody,
    ) -> Response;

    /// Serializes a modeled error with the protocol's discriminator framing.
    fn serialize_error(&self, error: &dyn HttpModeledError) -> Response;

    /// Converts a request-deserialization failure into the protocol's response.
    ///
    /// Each protocol answers with its `RuntimeError` responses, quirks included, such as awsJson
    /// and rpcv2Cbor collapsing `Accept` and `Content-Type` failures into a plain 400. These
    /// responses are the protocol's wire contract and must not change shape.
    fn serialize_rejection(&self, err: DeserializeError) -> Response;
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RequestBodyCollectionConfig {
    pub max_bytes: Option<NonZeroUsize>,
    pub read_timeout: Option<Duration>,
}

#[derive(Debug, Default, Clone)]
pub struct ServiceRequestBodyConfig {
    pub global: RequestBodyCollectionConfig,
    pub per_operation: HashMap<String, RequestBodyCollectionConfig>,
}

impl ServiceRequestBodyConfig {
    pub fn for_routing(&self) -> RequestBodyCollectionConfig {
        self.global
    }

    pub fn for_operation(&self, operation: &ShapeId<'_>) -> RequestBodyCollectionConfig {
        self.per_operation
            .get(operation.as_str())
            .copied()
            .unwrap_or(self.global)
    }
}

#[derive(Debug)]
pub enum RequestBodyCollectionError<E> {
    Body(E),
    TooLarge(crate::body::BodyLimitExceeded),
    Timeout { timeout: Duration },
}

impl<E: std::fmt::Display> std::fmt::Display for RequestBodyCollectionError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Body(err) => write!(f, "error reading request body: {err}"),
            Self::TooLarge(err) => err.fmt(f),
            Self::Timeout { timeout } => write!(f, "request body read timed out after {timeout:?}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for RequestBodyCollectionError<E> {}

pub async fn collect_request_body<B>(
    body: B,
    config: &RequestBodyCollectionConfig,
) -> Result<Bytes, RequestBodyCollectionError<B::Error>>
where
    B: HttpBody,
{
    let limit = config.max_bytes.map(NonZeroUsize::get).unwrap_or(0);
    let collect = async move {
        collect_body_limited(body, limit).await.map_err(|err| match err {
            CollectBodyError::Body(err) => RequestBodyCollectionError::Body(err),
            CollectBodyError::TooLarge(err) => RequestBodyCollectionError::TooLarge(err),
        })
    };
    match config.read_timeout {
        Some(timeout) => tokio::time::timeout(timeout, collect)
            .await
            .map_err(|_| RequestBodyCollectionError::Timeout { timeout })?,
        None => collect.await,
    }
}
