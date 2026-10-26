/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Schema-driven [`ServerProtocol`] implementations and protocol registration.
//!
//! [`ProtocolRegistry::BUILTIN`] documents the built-in claim priority order, how custom
//! protocols fit into it, and its effect on ambiguous requests.
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
mod rpc_v2_cbor_streaming;
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

use crate::body::BoxBody;
use crate::body::__private::{CollectBodyError, CollectBodyFuture, CollectionConfig, HttpBody, DEFAULT_FRAME_BUDGET};
use crate::response::Response;
use crate::schema::routing::RouterBuildError;
use crate::schema::OperationSchema;

pub use aws_json::{AwsJson1_0, AwsJson1_1};
pub use rest_json_1::RestJson1;
pub use rest_xml::RestXml;
pub use rpc_v2_cbor::RpcV2Cbor;

pub use registry::{ProtocolBuildContext, ProtocolFactory, ProtocolOrder, ProtocolRegistration, ProtocolRegistry};

use super::{DeserializeError, HttpModeledError};

/// Shared, erased server protocol selected by routing.
///
/// Wraps one of the routing kinds — [`metadata_routed`](Self::metadata_routed) or
/// [`body_routed`](Self::body_routed) — or a routing-less serde handle
/// ([`serde_only`](Self::serde_only)). The kind decides which
/// [`SharedProtocolRouter`](crate::schema::routing::SharedProtocolRouter) variant
/// is constructed and whether the protocol can answer the
/// event-stream question; everything after routing works through the [`ServerProtocol`] this
/// dereferences to. The kind stays private: the constructors' trait bounds are what guarantee
/// each variant builds the matching router, so nothing else may assemble one.
#[derive(Clone, Debug)]
pub struct SharedServerProtocol(ProtocolKind);

#[derive(Clone, Debug)]
enum ProtocolKind {
    /// Routes using request metadata alone and may support event-stream framing.
    Metadata(std::sync::Arc<dyn ErasedMetadataRoutedProtocol>),
    /// Routes non-streaming operations with access to collected request bodies when needed.
    BodyCollected(std::sync::Arc<dyn ErasedBodyRoutedProtocol>),
    /// Provides serialization and deserialization without routing or event-stream support.
    SerdeOnly(std::sync::Arc<dyn ServerProtocol>),
}

impl SharedServerProtocol {
    /// Borrows the protocol selected for this request from its extensions.
    ///
    /// Reads the protocol through [`SelectedOperation::get_from_request`](crate::schema::SelectedOperation::get_from_request).
    /// Returns `None` if no selection is stored, including before schema routing runs.
    pub fn get_from_request<B>(request: &http::Request<B>) -> Option<&Self> {
        super::SelectedOperation::get_from_request(request).map(super::SelectedOperation::protocol)
    }

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
    ) -> Result<Box<dyn crate::schema::routing::MetadataProtocolRouter>, RouterBuildError>;
    fn event_stream_framing(&self) -> Option<EventStreamFraming<'_>>;
}

impl<P: MetadataRoutedProtocol> ErasedMetadataRoutedProtocol for P {
    fn build_router(
        &self,
        ctx: crate::schema::routing::RouterBuildContext<'_>,
    ) -> Result<Box<dyn crate::schema::routing::MetadataProtocolRouter>, RouterBuildError> {
        Ok(Box::new(MetadataRoutedProtocol::build_router(self, ctx)?))
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
    ) -> Result<Box<dyn crate::schema::routing::BodyProtocolRouter>, RouterBuildError>;
}

impl<P: BodyRoutedProtocol> ErasedBodyRoutedProtocol for P {
    fn build_router(
        &self,
        ctx: crate::schema::routing::RouterBuildContext<'_>,
    ) -> Result<Box<dyn crate::schema::routing::BodyProtocolRouter>, RouterBuildError> {
        Ok(Box::new(BodyRoutedProtocol::build_router(self, ctx)?))
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
    ///
    /// This controls their payload and initial-request handling, not whether an explicitly
    /// requested initial-response frame is emitted.
    pub initial_messages_in_frames: bool,
    /// Initial-response payload when non-stream members travel in HTTP bindings instead.
    ///
    /// Used when [`initial_messages_in_frames`](Self::initial_messages_in_frames) is false.
    /// Legacy REST JSON sends `{}`; REST XML sends zero bytes.
    pub unset_initial_response_payload: &'static [u8],
    /// Optional body discriminator for modeled exception frames.
    pub exception_discriminator: Option<discriminator::BodyDiscriminator>,
    /// Whether HTTP-bound exception members are excluded from the frame payload.
    pub exception_http_bindings: bool,
    /// Codec for received exception frames that carry a REST XML error envelope, as legacy
    /// restXml servers expect. The codec reads the `<Error>` element without validating its
    /// root name.
    pub xml_exception_codec: Option<&'a dyn DynCodec>,
    /// With [`xml_exception_codec`](Self::xml_exception_codec): whether the `<Error>` element is
    /// wrapped in `<ErrorResponse>` (the default) or is the root (`@restXml(noErrorWrapping: true)`).
    pub xml_exceptions_wrapped: bool,
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
            unset_initial_response_payload: b"",
            exception_discriminator: None,
            exception_http_bindings: false,
            xml_exception_codec: None,
            xml_exceptions_wrapped: true,
        }
    }

    /// Reads received exception frames from a REST XML error envelope with `codec`, which must
    /// not validate the root element name: `<ErrorResponse><Error>..</Error></ErrorResponse>`
    /// when `wrapped`, otherwise `<Error>..</Error>`.
    pub fn xml_exceptions(mut self, codec: &'a dyn DynCodec, wrapped: bool) -> Self {
        self.xml_exception_codec = Some(codec);
        self.xml_exceptions_wrapped = wrapped;
        self
    }

    /// Sets the body discriminator used for modeled exception payloads.
    pub fn exception_discriminator(mut self, discriminator: discriminator::BodyDiscriminator) -> Self {
        self.exception_discriminator = Some(discriminator);
        self
    }

    /// Applies REST error-body bindings to structured exception payloads.
    pub fn exception_http_bindings(mut self, enabled: bool) -> Self {
        self.exception_http_bindings = enabled;
        self
    }

    /// Sets whether non-stream members travel in initial-message frames.
    pub fn initial_messages_in_frames(mut self, initial_messages_in_frames: bool) -> Self {
        self.initial_messages_in_frames = initial_messages_in_frames;
        self
    }

    /// Sets the initial-response payload for protocols whose non-stream members use HTTP
    /// bindings. Defaults to zero bytes; ignored when initial messages carry output members.
    pub fn unset_initial_response_payload(mut self, payload: &'static [u8]) -> Self {
        self.unset_initial_response_payload = payload;
        self
    }
}

/// Schema-driven serialization for one protocol, keyed on struct schemas.
///
/// The request side takes the operation's input schema, the response side its output schema,
/// mirroring the client's `ClientProtocolInner` with the two directions swapped. The trait is
/// object-safe: routing stores the protocol in a private request-extension carrier for
/// [`SelectedOperation`](crate::schema::SelectedOperation). Everything after routing works
/// through that erased handle, so nothing downstream names a concrete protocol.
///
/// A response plan is precomputed per operation: Bindings, Media types, URI templates
/// and status codes are cached.
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
/// use aws_smithy_http_server::schema::SharedServerProtocol;
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
///     let protocol = SharedServerProtocol::get_from_request(request)?;
///     let mut response = protocol.serialize_response(&TEAPOT, &Teapot);
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
    /// Event-stream inputs bypass collection and receive the live body separately.
    /// Streaming blob inputs honor `Collect`: the codec reads the blob and ordinary fields
    /// before handler dispatch. With `Skip`, streaming glue receives the live body separately.
    fn request_body_requirement(&self, _operation: &OperationSchema<'_>) -> BodyDirective {
        BodyDirective::Collect
    }

    /// Presents `request` as a deserializer for `input`.
    ///
    /// The `Content-Type` check happens here; the returned deserializer resolves `@http` bindings
    /// from the request and hands body members to its internal codec. Synchronous over an
    /// already collected body. The request retains HTTP metadata, including extensions.
    /// Its body is empty when collection is skipped or the input is an event stream;
    /// streaming glue receives the live body separately. Collected streaming blobs are
    /// decoded from the document and exposed to handlers as buffered byte streams.
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
    ///
    /// Built-in protocols add the buffered body's byte count as `Content-Length` only when
    /// no modeled value is present. Matching legacy generated responses, modeled values take
    /// precedence without length-specific validation or normalization.
    fn serialize_response(&self, output: &Schema<'_>, value: &dyn SerializableStruct) -> Response;

    /// Serializes a streaming response for `output` around a caller-supplied `body`.
    ///
    /// The protocol interprets `output` and `value` to produce the response head and any
    /// ordinary body members. Generated glue builds `body` from the streaming blob or
    /// preframed event stream. CBOR frames blob chunks after the ordinary serialized members.
    ///
    /// `None` means the modeled streaming member is absent. `Some(body)` means it is
    /// present, even when the body yields no bytes. CBOR must distinguish these cases:
    /// absence omits the blob member; a present empty blob encodes an explicit empty byte string.
    /// REST and event-stream responses use an empty payload when the member is absent.
    /// The body is passed separately because `serialize_members` omits streaming members
    /// and the response must own their streams.
    ///
    /// Built-in protocols do not infer a length or collect the stream. Matching legacy generated
    /// responses, modeled `Content-Length` values are preserved without length-specific validation
    /// or normalization.
    fn serialize_streaming_response(
        &self,
        output: &Schema<'_>,
        value: &dyn SerializableStruct,
        streaming_member: &Schema<'_>,
        body: Option<BoxBody>,
    ) -> Response;

    /// Serializes a modeled error with the protocol's discriminator framing.
    fn serialize_error(&self, error: &dyn HttpModeledError) -> Response;

    /// Frames a shared extractor runtime error using this protocol's runtime-error contract.
    ///
    /// Schema upgrades supply the protocol selected for the request. This is distinct from
    /// modeled constraint validation and must preserve the protocol's existing runtime error
    /// status, headers, body, and response extensions. External protocols implement this
    /// method alongside [`Self::serialize_rejection`] to define their own framing.
    fn serialize_runtime_error(&self, error: crate::runtime_error::RuntimeError) -> Response;

    /// Converts a routing rejection into this protocol's response.
    ///
    /// The routing service classifies why routing failed
    /// ([`RoutingError::kind`](crate::schema::routing::RoutingError::kind)); the protocol owns the
    /// wire form. The default keeps the historical smithy-rs responses — the rejection is a
    /// member-less modeled error (`UnknownOperationException` at `404`,
    /// `MethodNotAllowedException` at `405`) framed by [`serialize_error`](Self::serialize_error).
    /// A protocol whose clients expect different routing error responses overrides this;
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

/// Converts a body collection failure into the protocol's rejection response, retaining
/// legacy wire behavior: the failure surfaces as an ordinary request-deserialization error.
pub(crate) fn body_collection_rejection(
    protocol: &dyn ServerProtocol,
    error: RequestBodyCollectionError<crate::Error>,
) -> Response {
    protocol.serialize_rejection(error.into())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RequestBodyCollectionConfig {
    pub max_bytes: Option<NonZeroUsize>,
    pub read_timeout: Option<Duration>,
    /// Consecutive ready HTTP body frame polls before yielding, for collection and streaming.
    /// Defaults to 32; `None` disables forced yielding.
    pub read_poll_budget: Option<NonZeroUsize>,
}

impl Default for RequestBodyCollectionConfig {
    fn default() -> Self {
        Self {
            max_bytes: None,
            read_timeout: None,
            read_poll_budget: Some(DEFAULT_FRAME_BUDGET),
        }
    }
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

    /// Sets the frame poll budget for collection and streaming; `None` disables forced yielding.
    pub fn with_read_poll_budget(mut self, read_poll_budget: Option<NonZeroUsize>) -> Self {
        self.read_poll_budget = read_poll_budget;
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
            maximum.read_poll_budget = match (maximum.read_poll_budget, config.read_poll_budget) {
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
    BodyLimitExceeded(NonZeroUsize),
    Timeout { timeout: Duration },
}

impl<E> RequestBodyCollectionError<E> {
    /// Maps a transport-specific error without changing collection-limit failures.
    pub fn map_body_error<F>(self, map: impl FnOnce(E) -> F) -> RequestBodyCollectionError<F> {
        match self {
            Self::Body(error) => RequestBodyCollectionError::Body(map(error)),
            Self::BodyLimitExceeded(limit) => RequestBodyCollectionError::BodyLimitExceeded(limit),
            Self::Timeout { timeout } => RequestBodyCollectionError::Timeout { timeout },
        }
    }
}

impl<E: std::fmt::Display> std::fmt::Display for RequestBodyCollectionError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Body(err) => write!(f, "error reading request body: {err}"),
            Self::BodyLimitExceeded(limit) => {
                write!(f, "request body exceeded the configured maximum of {limit} bytes")
            }
            Self::Timeout { timeout } => write!(f, "request body read timed out after {timeout:?}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for RequestBodyCollectionError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Body(error) => Some(error),
            Self::BodyLimitExceeded(_) | Self::Timeout { .. } => None,
        }
    }
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
            return Err(RequestBodyCollectionError::BodyLimitExceeded(limit));
        }
        return Ok(bytes.clone());
    }
    collect_request_body_with_trailers(body, config)
        .await
        .map(|(bytes, _)| bytes)
}

/// Shared complete-body collection for routing and operation deserialization.
pub(crate) fn collect_request_body_with_trailers<B>(
    body: B,
    config: &RequestBodyCollectionConfig,
) -> RequestBodyCollectionFuture<B>
where
    B: HttpBody,
{
    RequestBodyCollectionFuture {
        inner: CollectionState::Start {
            body: Some(body),
            config: *config,
        },
    }
}

pin_project_lite::pin_project! {
    #[project = CollectionStateProj]
    enum CollectionState<B> {
        Start {
            body: Option<B>,
            config: RequestBodyCollectionConfig,
        },
        Collect {
            #[pin]
            future: CollectBodyFuture<B, true>,
        },
        Timed {
            #[pin]
            // Tokio's timer cannot move once pinned. Keep the timed branch boxed so the
            // public routing future retains its existing Unpin implementation.
            future: std::pin::Pin<Box<tokio::time::Timeout<CollectBodyFuture<B, true>>>>,
            timeout: Duration,
        },
        Done,
    }
}

pin_project_lite::pin_project! {
    /// A named collection future so routing can store body I/O without a box.
    pub(crate) struct RequestBodyCollectionFuture<B> {
        #[pin]
        inner: CollectionState<B>,
    }
}

impl<B: HttpBody> std::future::Future for RequestBodyCollectionFuture<B> {
    type Output = Result<(Bytes, Option<http::HeaderMap>), RequestBodyCollectionError<B::Error>>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        use std::task::Poll;
        let mut this = self.project();
        loop {
            let result = match this.inner.as_mut().project() {
                CollectionStateProj::Start { body, config } => {
                    let future = CollectBodyFuture::<B, true>::new(
                        body.take().expect("collection starts once"),
                        Some(
                            CollectionConfig::builder()
                                .set_limit(config.max_bytes)
                                .set_frame_budget(config.read_poll_budget)
                                .build(),
                        ),
                    );
                    let state = match config.read_timeout {
                        Some(timeout) => CollectionState::Timed {
                            future: Box::pin(tokio::time::timeout(timeout, future)),
                            timeout,
                        },
                        None => CollectionState::Collect { future },
                    };
                    this.inner.set(state);
                    continue;
                }
                CollectionStateProj::Collect { future } => std::task::ready!(future.poll(cx)),
                CollectionStateProj::Timed { future, timeout } => match std::task::ready!(future.poll(cx)) {
                    Ok(result) => result,
                    Err(_) => {
                        let timeout = *timeout;
                        this.inner.set(CollectionState::Done);
                        return Poll::Ready(Err(RequestBodyCollectionError::Timeout { timeout }));
                    }
                },
                CollectionStateProj::Done => panic!("polled after completion"),
            };
            this.inner.set(CollectionState::Done);
            return Poll::Ready(result.map_err(|err| match err {
                CollectBodyError::Body(err) => RequestBodyCollectionError::Body(err),
                CollectBodyError::BodyLimitExceeded(limit) => RequestBodyCollectionError::BodyLimitExceeded(limit),
            }));
        }
    }
}
