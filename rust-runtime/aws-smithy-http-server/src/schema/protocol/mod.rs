/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Schema-driven [`ServerProtocol`] implementations and protocol registration.
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
pub(crate) mod request;
#[doc(hidden)]
pub mod response;
pub(crate) mod rest;
mod rest_json_1;
mod rest_xml;
#[doc(hidden)]
pub mod rpc;
mod rpc_v2_cbor;
pub(crate) mod rpc_v2_cbor_serde;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::time::Duration;

use aws_smithy_runtime_api::http::Headers;
use aws_smithy_schema::codec::DynCodec;
use aws_smithy_schema::serde::{SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{Schema, ShapeId};
use bytes::Bytes;

use crate::body::{collect_body_limited_with_trailers, BoxBody, CollectBodyError, HttpBody};
use crate::response::Response;
use crate::schema::routing::RouterBuildError;
use crate::schema::OperationSchema;

pub use aws_json::{AwsJson1_0Protocol, AwsJson1_1Protocol};
pub use rest_json_1::RestJson1Protocol;
pub use rest_xml::RestXmlProtocol;
pub use rpc_v2_cbor::RpcV2CborProtocol;

pub use registry::{ProtocolBuildContext, ProtocolFactory, ProtocolOrder, ProtocolRegistration, ProtocolRegistry};

use super::{DeserializeError, HttpModeledError};

/// Shared, erased server protocol selected by routing.
///
/// Wraps one of the routing kinds — [`metadata_routed`](Self::metadata_routed) or
/// [`body_routed`](Self::body_routed) — or a routing-less serde handle
/// ([`serde_only`](Self::serde_only)). The kind decides which
/// [`SharedProtocolRouter`](crate::schema::routing::SharedProtocolRouter) variant
/// [`build_router`](Self::build_router) produces and whether the protocol can answer the
/// event-stream question; everything after routing works through the [`ServerProtocol`] this
/// dereferences to. The kind stays private: the constructors' trait bounds are what guarantee
/// each variant builds the matching router, so nothing else may assemble one.
#[derive(Clone, Debug)]
pub struct SharedServerProtocol(ProtocolKind);

#[derive(Clone, Debug)]
enum ProtocolKind {
    Metadata(std::sync::Arc<dyn ErasedMetadataRoutedProtocol>),
    BodyCollected(std::sync::Arc<dyn ErasedBodyRoutedProtocol>),
    SerdeOnly(std::sync::Arc<dyn ServerProtocol>),
}

impl SharedServerProtocol {
    /// Wraps a protocol that selects operations from request metadata alone.
    pub fn metadata_routed(protocol: impl MetadataRoutedProtocol) -> Self {
        Self(ProtocolKind::Metadata(std::sync::Arc::new(protocol)))
    }

    /// Wraps a protocol that reads the request body to select operations.
    ///
    /// A body-routed protocol cannot express event-stream framing — the subtrait has no such
    /// method — so this handle always answers `None` from [`Self::event_stream_framing`].
    pub fn body_routed(protocol: impl BodyRoutedProtocol) -> Self {
        Self(ProtocolKind::BodyCollected(std::sync::Arc::new(protocol)))
    }

    /// Wraps a protocol for serialization only, without routing or event-stream support.
    ///
    /// For handles that never route: middleware serializing through a hand-built protocol, and
    /// tests. Registration requires one of the routed constructors, so this handle is never
    /// asked to build a router.
    pub fn serde_only(protocol: impl ServerProtocol) -> Self {
        Self(ProtocolKind::SerdeOnly(std::sync::Arc::new(protocol)))
    }

    /// Builds this protocol's operation router; see
    /// [`MetadataRoutedProtocol::build_router`] and [`BodyRoutedProtocol::build_router`].
    ///
    /// `ctx.targets` carries every bound operation; `non_streaming` the subset without
    /// streaming members. The kind picks here: a body-routed protocol may buffer the body to
    /// select, so a streaming operation is never its to claim and its router is built without
    /// them.
    pub(crate) fn build_router<'a>(
        &self,
        mut ctx: crate::schema::routing::RouterBuildContext<'a>,
        non_streaming: &'a [crate::schema::routing::OperationTarget],
    ) -> Result<crate::schema::routing::SharedProtocolRouter, RouterBuildError> {
        use crate::schema::routing::SharedProtocolRouter;
        match &self.0 {
            ProtocolKind::Metadata(protocol) => Ok(SharedProtocolRouter::Metadata(protocol.build_router(ctx)?)),
            ProtocolKind::BodyCollected(protocol) => {
                ctx.targets = non_streaming;
                Ok(SharedProtocolRouter::Body(protocol.build_router(ctx)?))
            }
            ProtocolKind::SerdeOnly(protocol) => Err(RouterBuildError::Configuration(format!(
                "protocol {} was wrapped without routing support",
                protocol.protocol_id()
            ))),
        }
    }

    /// Event-frame support, when this protocol supports event streams.
    ///
    /// Only a metadata-routed protocol can answer: the [`BodyRoutedProtocol`] subtrait has no
    /// framing method (an event-stream body must not be collected), and a serde-only handle
    /// never routes.
    pub fn event_stream_framing(&self) -> Option<EventStreamFraming<'_>> {
        match &self.0 {
            ProtocolKind::Metadata(protocol) => protocol.event_stream_framing(),
            ProtocolKind::BodyCollected(_) | ProtocolKind::SerdeOnly(_) => None,
        }
    }
}

impl std::ops::Deref for SharedServerProtocol {
    type Target = dyn ServerProtocol;

    fn deref(&self) -> &Self::Target {
        match &self.0 {
            ProtocolKind::Metadata(protocol) => protocol.as_ref(),
            ProtocolKind::BodyCollected(protocol) => protocol.as_ref(),
            ProtocolKind::SerdeOnly(protocol) => protocol.as_ref(),
        }
    }
}

/// Object-safe facade over [`MetadataRoutedProtocol`], which cannot be a trait object itself:
/// its `from_build_context` returns `Self` and its `build_router` returns
/// `impl MetadataProtocolRouter`. Blanket-implemented for every metadata-routed protocol, so
/// the one boxing step lives here and [`ProtocolKind::Metadata`] holds the protocol directly.
trait ErasedMetadataRoutedProtocol: ServerProtocol {
    fn build_router(
        &self,
        ctx: crate::schema::routing::RouterBuildContext<'_>,
    ) -> Result<std::sync::Arc<dyn crate::schema::routing::MetadataProtocolRouter>, RouterBuildError>;
    fn event_stream_framing(&self) -> Option<EventStreamFraming<'_>>;
}

impl<P: MetadataRoutedProtocol> ErasedMetadataRoutedProtocol for P {
    fn build_router(
        &self,
        ctx: crate::schema::routing::RouterBuildContext<'_>,
    ) -> Result<std::sync::Arc<dyn crate::schema::routing::MetadataProtocolRouter>, RouterBuildError> {
        Ok(std::sync::Arc::new(MetadataRoutedProtocol::build_router(self, ctx)?))
    }
    fn event_stream_framing(&self) -> Option<EventStreamFraming<'_>> {
        MetadataRoutedProtocol::event_stream_framing(self)
    }
}

/// Object-safe facade over [`BodyRoutedProtocol`], exactly as [`ErasedMetadataRoutedProtocol`] is
/// over [`MetadataRoutedProtocol`].
trait ErasedBodyRoutedProtocol: ServerProtocol {
    fn build_router(
        &self,
        ctx: crate::schema::routing::RouterBuildContext<'_>,
    ) -> Result<std::sync::Arc<dyn crate::schema::routing::BodyProtocolRouter>, RouterBuildError>;
}

impl<P: BodyRoutedProtocol> ErasedBodyRoutedProtocol for P {
    fn build_router(
        &self,
        ctx: crate::schema::routing::RouterBuildContext<'_>,
    ) -> Result<std::sync::Arc<dyn crate::schema::routing::BodyProtocolRouter>, RouterBuildError> {
        Ok(std::sync::Arc::new(BodyRoutedProtocol::build_router(self, ctx)?))
    }
}

/// Whether request deserialization needs the body to be collected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BodyDirective {
    /// Collect the body under the operation's limits (the default).
    Collect,
    /// Skip collection; the input never reads the body.
    Skip,
}

/// The event-frame capability of a metadata-routed protocol, as plain data.
///
/// Answered by [`MetadataRoutedProtocol::event_stream_framing`]; a protocol that does not
/// support event streams answers `None`. Body-routed protocols cannot express this at all:
/// body-first routing collects the body before selecting an operation, and an event-stream
/// body must not be collected.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct EventStreamFraming<'a> {
    /// Codec for structured event and initial-message payloads.
    pub payload_codec: &'a dyn DynCodec,
    /// Media type of structured event payloads.
    pub media_type: &'a str,
    /// Whether non-stream members travel in initial-message frames.
    pub initial_messages_in_frames: bool,
}

impl<'a> EventStreamFraming<'a> {
    /// Framing with the given payload codec and media type; non-stream members do not travel
    /// in initial-message frames unless [`initial_messages_in_frames`](Self::initial_messages_in_frames)
    /// is set.
    pub fn new(payload_codec: &'a dyn DynCodec, media_type: &'a str) -> Self {
        Self {
            payload_codec,
            media_type,
            initial_messages_in_frames: false,
        }
    }

    /// Sets whether non-stream members travel in initial-message frames.
    pub fn initial_messages_in_frames(mut self, initial_messages_in_frames: bool) -> Self {
        self.initial_messages_in_frames = initial_messages_in_frames;
        self
    }
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

    /// Validates request headers for the selected operation before upgrade body collection
    /// and [`Self::deserialize_request`]. Header failures therefore take precedence over
    /// deserialization failures. The built-in protocols check `Accept` against the response
    /// content type. The default accepts all headers.
    fn validate_request_headers(
        &self,
        _operation: &OperationSchema<'_>,
        _headers: &Headers,
    ) -> Result<(), DeserializeError> {
        Ok(())
    }

    /// Specifies whether deserialization needs the request body. The default collects it.
    ///
    /// Returning [`BodyDirective::Skip`] leaves the deserialization request's body empty.
    /// Streaming inputs bypass collection regardless of this requirement; their live body
    /// is passed separately to the generated streaming glue.
    fn request_body_requirement(&self, _operation: &OperationSchema<'_>) -> BodyDirective {
        BodyDirective::Collect
    }

    /// Presents `request` as a deserializer for `input`.
    ///
    /// The `Content-Type` check happens here; the returned deserializer resolves `@http` bindings
    /// from the request and hands body members to its internal codec. Synchronous over an
    /// already collected body. The request retains HTTP metadata, including extensions.
    /// Its body is empty when collection is skipped or the input streams; streaming glue
    /// receives the live body separately.
    fn deserialize_request<'a>(
        &'a self,
        input: &Schema<'_>,
        request: &'a aws_smithy_runtime_api::http::Request<bytes::Bytes>,
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

    /// Converts a routing rejection into this protocol's response.
    ///
    /// The routing service classifies why routing failed
    /// ([`RoutingError::kind`](crate::schema::routing::RoutingError::kind)); the protocol owns the
    /// wire form. The default keeps the historical smithy-rs responses — the rejection is a
    /// member-less modeled error (`UnknownOperationException` at `404`,
    /// `MethodNotAllowedException` at `405`) framed by [`serialize_error`](Self::serialize_error).
    /// A protocol whose clients expect different bytes (e.g. Coral parity) overrides this;
    /// returning a full [`Response`] permits any status, header — including
    /// `Connection: close` — and body.
    fn serialize_routing_error(&self, err: &crate::schema::routing::RoutingError) -> Response {
        self.serialize_error(err)
    }

    /// Converts a rejected request into the protocol's response.
    ///
    /// Each protocol answers with its `RuntimeError` responses, quirks included, such as awsJson
    /// and rpcv2Cbor collapsing `Accept` and `Content-Type` failures into a plain 400, and
    /// [`DeserializeError::InternalFailure`] becoming the 500 internal-failure response. These
    /// responses are the protocol's wire contract and must not change shape.
    fn serialize_rejection(&self, err: DeserializeError) -> Response;
}

/// A [`ServerProtocol`] that selects operations from request metadata alone.
///
/// Registered with [`ProtocolRegistration::metadata_routed`]; the registry calls
/// [`from_build_context`](Self::from_build_context) only for services whose schema declares the
/// registered protocol trait, and the routing service calls
/// [`build_router`](Self::build_router) once per service. Only a metadata-routed protocol may
/// support event streams: routing never collects the body, so an event-stream body is never
/// read before its operation is known.
pub trait MetadataRoutedProtocol: ServerProtocol + Sized {
    /// Builds this protocol for a service, from its registered configuration. Invalid
    /// configuration fails the service build.
    fn from_build_context(ctx: &ProtocolBuildContext<'_>) -> Result<Self, RouterBuildError>;

    /// Builds operation routing once for this service.
    ///
    /// The context carries the protocol's own settings section next to the server-global
    /// configuration; invalid settings are rejected with
    /// [`RouterBuildError::Configuration`], failing the service build.
    ///
    /// `use<Self>` pins down the capture contract: the returned router owns its state and
    /// borrows nothing from the protocol or the context, so a caller may build a router from
    /// a temporary protocol. Implementations write `use<>` in their return type.
    fn build_router(
        &self,
        ctx: crate::schema::routing::RouterBuildContext<'_>,
    ) -> Result<impl crate::schema::routing::MetadataProtocolRouter + 'static + use<Self>, RouterBuildError>;

    /// Event-frame support, when this protocol supports event streams.
    fn event_stream_framing(&self) -> Option<EventStreamFraming<'_>> {
        None
    }
}

/// A [`ServerProtocol`] that reads the request body to select operations.
///
/// Registered with [`ProtocolRegistration::body_routed`]. Claiming is head-first; the router
/// escalates to body bytes by returning a requirement the routing service satisfies — the
/// service owns all body I/O, the protocol owns interpretation. The protocol is handed only
/// the service's non-streaming operations — see
/// [`BodyProtocolRouter`](crate::schema::routing::BodyProtocolRouter). Event-stream framing is not
/// expressible here: the subtrait has no such method, so the conflict between body-based
/// routing and serving event-stream bodies cannot arise. Metadata routers can recognize
/// streaming inputs so claims requiring body bytes can be skipped.
pub trait BodyRoutedProtocol: ServerProtocol + Sized {
    /// Builds this protocol for a service, from its registered configuration. Invalid
    /// configuration fails the service build.
    fn from_build_context(ctx: &ProtocolBuildContext<'_>) -> Result<Self, RouterBuildError>;

    /// Builds operation routing once for this service, over the service's non-streaming
    /// operations.
    ///
    /// `use<Self>` pins down the capture contract exactly as on
    /// [`MetadataRoutedProtocol::build_router`]; implementations write `use<>`.
    fn build_router(
        &self,
        ctx: crate::schema::routing::RouterBuildContext<'_>,
    ) -> Result<impl crate::schema::routing::BodyProtocolRouter + 'static + use<Self>, RouterBuildError>;
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
) -> Result<bool, crate::schema::routing::RouterBuildError> {
    let Some(settings) = settings else {
        return Ok(false);
    };
    let aws_smithy_types::Document::Object(object) = settings else {
        return Err(crate::schema::routing::RouterBuildError::Configuration(format!(
            "protocol settings must be a JSON object, got {settings:?}"
        )));
    };
    match object.get(key) {
        None => Ok(false),
        Some(aws_smithy_types::Document::Bool(value)) => Ok(*value),
        Some(other) => Err(crate::schema::routing::RouterBuildError::Configuration(format!(
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
#[non_exhaustive]
pub struct RequestBodyCollectionConfig {
    pub max_bytes: Option<NonZeroUsize>,
    pub read_timeout: Option<Duration>,
}

#[derive(Debug, Default, Clone)]
#[non_exhaustive]
pub struct ServiceRequestBodyConfig {
    pub global: RequestBodyCollectionConfig,
    pub per_operation: HashMap<String, RequestBodyCollectionConfig>,
}

impl RequestBodyCollectionConfig {
    /// Sets the byte limit; `None` allows any size.
    pub fn with_max_bytes(mut self, max_bytes: Option<NonZeroUsize>) -> Self {
        self.max_bytes = max_bytes;
        self
    }

    /// Sets the read timeout; `None` disables it.
    pub fn with_read_timeout(mut self, read_timeout: Option<Duration>) -> Self {
        self.read_timeout = read_timeout;
        self
    }
}

impl ServiceRequestBodyConfig {
    /// Sets the default body-read allowances.
    pub fn with_global(mut self, global: RequestBodyCollectionConfig) -> Self {
        self.global = global;
        self
    }

    /// Sets operation overrides, keyed by operation shape ID.
    pub fn with_per_operation(mut self, per_operation: HashMap<String, RequestBodyCollectionConfig>) -> Self {
        self.per_operation = per_operation;
        self
    }

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
#[non_exhaustive]
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
    collect_request_body_with_trailers(body, config)
        .await
        .map(|(bytes, _)| bytes)
}

/// Shared complete-body collection for routing and operation deserialization.
pub(crate) async fn collect_request_body_with_trailers<B>(
    body: B,
    config: &RequestBodyCollectionConfig,
) -> Result<(Bytes, Option<http::HeaderMap>), RequestBodyCollectionError<B::Error>>
where
    B: HttpBody,
{
    let limit = config.max_bytes.map(NonZeroUsize::get).unwrap_or(0);
    let collect = async move {
        collect_body_limited_with_trailers(body, limit)
            .await
            .map_err(|err| match err {
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
