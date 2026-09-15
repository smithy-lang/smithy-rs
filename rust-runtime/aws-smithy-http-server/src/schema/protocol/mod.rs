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
// The `discriminator`, `response` and `rpc` modules are `#[doc(hidden)]` seams for
// `ServerProtocol` implementations living outside this crate. They are not part of the crate's
// stable API: no semver guarantee, subject to change with the in-tree protocols that share them.
#[doc(hidden)]
pub mod discriminator;
mod registry;
mod request;
#[doc(hidden)]
pub mod response;
pub(crate) mod rest;
pub(crate) mod rest_json_1;
pub(crate) mod rest_xml;
#[doc(hidden)]
pub mod rpc;
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

pub use registry::{ProtocolRegistration, ProtocolRegistry};

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
    /// Builds operation routing once for this service.
    ///
    /// The context carries the protocol's own settings section next to the
    /// server-global configuration; invalid settings are rejected with
    /// [`RouterBuildError::Configuration`](crate::routing::RouterBuildError::Configuration),
    /// failing the service build.
    fn build_router(
        &self,
        ctx: crate::routing::RouterBuildContext<'_>,
    ) -> Result<crate::routing::SharedProtocolRouter, crate::routing::RouterBuildError>;

    /// Renders the protocol's existing internal failure response when a service is built
    /// using build_unchecked() and a handler has not been set.
    fn serialize_internal_failure(&self) -> Response;

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

/// Parses a JSON object emitted by codegen into a settings [`Document`].
///
/// Generated `routing_options()` functions embed each protocol's section of
/// `customizationConfig.protocols` as a JSON byte-string and call this once at
/// service build time. The input is printed by codegen from a validated node,
/// so malformed JSON is a codegen bug: this panics rather than returning an
/// error.
///
/// [`Document`]: aws_smithy_types::Document
pub fn parse_settings_json(json: &[u8]) -> aws_smithy_types::Document {
    let mut tokens = aws_smithy_json::deserialize::json_token_iter(json).peekable();
    let document = aws_smithy_json::deserialize::token::expect_document(&mut tokens)
        .expect("codegen emits well-formed settings JSON");
    assert!(tokens.next().is_none(), "codegen emits a single settings JSON document");
    document
}

/// Reads an opt-in boolean flag from a protocol's settings section.
///
/// Absent section or key is `false`. A section that is not an object, or a
/// value that is not a boolean, is a configuration error.
pub fn settings_bool(
    settings: Option<&aws_smithy_types::Document>,
    key: &str,
) -> Result<bool, crate::routing::RouterBuildError> {
    let Some(settings) = settings else {
        return Ok(false);
    };
    let aws_smithy_types::Document::Object(object) = settings else {
        return Err(crate::routing::RouterBuildError::Configuration(format!(
            "protocol settings must be a JSON object, got {settings:?}"
        )));
    };
    match object.get(key) {
        None => Ok(false),
        Some(aws_smithy_types::Document::Bool(value)) => Ok(*value),
        Some(other) => Err(crate::routing::RouterBuildError::Configuration(format!(
            "protocol setting `{key}` must be a boolean, got {other:?}"
        ))),
    }
}

/// Converts a body collection failure into the protocol's rejection response, retaining
/// legacy wire behavior: the failure surfaces as an ordinary request-deserialization error.
pub(crate) fn body_collection_rejection(
    protocol: &dyn ServerProtocol,
    error: RequestBodyCollectionError<crate::Error>,
) -> Response {
    protocol.serialize_rejection(DeserializeError::Serde(aws_smithy_schema::serde::SerdeError::custom(
        error.to_string(),
    )))
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
    /// Computes the provisional allowance before the operation is known.
    /// An absent operation override inherits `global`; an unlimited field dominates the maximum.
    pub fn for_routing(&self) -> RequestBodyCollectionConfig {
        let mut maximum = self.global;
        for config in self.per_operation.values() {
            maximum.max_bytes = match (maximum.max_bytes, config.max_bytes) {
                (Some(left), Some(right)) => Some(left.max(right)),
                _ => None,
            };
            maximum.read_timeout = match (maximum.read_timeout, config.read_timeout) {
                (Some(left), Some(right)) => Some(left.max(right)),
                _ => None,
            };
        }
        maximum
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

impl<E> RequestBodyCollectionError<E> {
    /// Maps a transport-specific error without changing collection-limit failures.
    pub fn map_body_error<F>(self, map: impl FnOnce(E) -> F) -> RequestBodyCollectionError<F> {
        match self {
            Self::Body(error) => RequestBodyCollectionError::Body(map(error)),
            Self::TooLarge(error) => RequestBodyCollectionError::TooLarge(error),
            Self::Timeout { timeout } => RequestBodyCollectionError::Timeout { timeout },
        }
    }
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

/// Collects a request body for body-first routing, returning the content routing selects from,
/// trailers included.
///
/// This is the collection step of an [`AsyncProtocolRouter`]: the router selects an operation
/// from the returned bytes and hands the [`CollectedBody`] back with its selection; the routing
/// service rebuilds the dispatched request around it, so the handler reads exactly what routing
/// read. The allowance in `config` is enforced during collection — protocols derive it from
/// [`RouterBuildContext::config`] with [`ServiceRequestBodyConfig::for_routing`] when
/// building their router — and a failure is framed by the protocol itself.
///
/// [`AsyncProtocolRouter`]: crate::routing::AsyncProtocolRouter
/// [`CollectedBody`]: crate::routing::CollectedBody
/// [`RouterBuildContext::config`]: crate::routing::RouterBuildContext
pub async fn collect_for_routing(
    body: BoxBody,
    config: &RequestBodyCollectionConfig,
) -> Result<crate::routing::CollectedBody, RequestBodyCollectionError<crate::Error>> {
    let collect = collect_frames(body, config);
    let (bytes, trailers) = match config.read_timeout {
        Some(timeout) => tokio::time::timeout(timeout, collect)
            .await
            .map_err(|_| RequestBodyCollectionError::Timeout { timeout })??,
        None => collect.await?,
    };
    Ok(crate::routing::CollectedBody { bytes, trailers })
}

/// Collects data frames under the size allowance, retaining trailers.
async fn collect_frames(
    body: BoxBody,
    config: &RequestBodyCollectionConfig,
) -> Result<(Bytes, Option<http::HeaderMap>), RequestBodyCollectionError<crate::Error>> {
    let mut body = std::pin::pin!(body);
    let mut bytes = bytes::BytesMut::new();
    let mut trailers: Option<http::HeaderMap> = None;
    while let Some(frame) = std::future::poll_fn(|cx| body.as_mut().poll_frame(cx))
        .await
        .transpose()
        .map_err(RequestBodyCollectionError::Body)?
    {
        match frame.into_data() {
            Ok(data) => {
                if let Some(limit) = config.max_bytes {
                    if data.len() > limit.get().saturating_sub(bytes.len()) {
                        return Err(RequestBodyCollectionError::TooLarge(crate::body::BodyLimitExceeded {
                            limit: limit.get(),
                        }));
                    }
                }
                bytes.extend_from_slice(&data);
            }
            Err(frame) => {
                if let Ok(new_trailers) = frame.into_trailers() {
                    trailers.get_or_insert_with(http::HeaderMap::new).extend(new_trailers);
                }
            }
        }
    }
    Ok((bytes.freeze(), trailers))
}

pub async fn collect_request_body<B>(
    body: B,
    config: &RequestBodyCollectionConfig,
) -> Result<Bytes, RequestBodyCollectionError<B::Error>>
where
    B: HttpBody + 'static,
{
    if let Some(bytes) = (&body as &dyn std::any::Any)
        .downcast_ref::<crate::body::Body>()
        .and_then(crate::body::Body::buffered_content)
    {
        if let Some(limit) = config.max_bytes.filter(|limit| bytes.len() > limit.get()) {
            return Err(RequestBodyCollectionError::TooLarge(crate::body::BodyLimitExceeded {
                limit: limit.get(),
            }));
        }
        return Ok(bytes.clone());
    }
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
