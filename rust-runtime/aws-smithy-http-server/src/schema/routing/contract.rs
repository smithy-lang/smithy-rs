/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The protocol-facing routing contract: router traits, claims, and build-time types.

use crate::routing::SyncRoute;
use crate::{
    error::BoxError,
    schema::ServiceRequestBodyConfig,
};
use crate::schema::{OperationSchema, ServiceSchema};
use crate::schema::routing::RoutingError;
use aws_smithy_types::Document;
use bytes::Bytes;
use http::Request;

use std::{
    collections::HashMap,
    fmt,
    sync::Arc,
};

/// The kind of streaming member in an operation's input or output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamingKind {
    /// A streaming blob payload.
    Blob,
    /// A streaming union of events.
    EventStream,
}

/// Protocol-independent facts derived once from an operation's canonical schema.
#[derive(Clone, Copy, Debug)]
struct OperationMetadata {
    input_streaming: Option<StreamingKind>,
    output_streaming: Option<StreamingKind>,
}
impl OperationMetadata {
    fn new(operation: &OperationSchema<'_>) -> Self {
        fn streaming_kind(schema: &aws_smithy_schema::Schema<'_>) -> Option<StreamingKind> {
            schema
                .members()
                .iter()
                .find(|member| member.streaming())
                .map(|member| {
                    // Smithy streaming members are blobs or event-stream unions.
                    if member.shape_type() == aws_smithy_schema::ShapeType::Blob {
                        StreamingKind::Blob
                    } else {
                        StreamingKind::EventStream
                    }
                })
        }
        Self {
            input_streaming: streaming_kind(operation.input()),
            output_streaming: streaming_kind(operation.output()),
        }
    }
}

/// A router's operation target: its canonical schema, cached metadata and handler position.
///
/// Targets are assigned by the routing service and passed to protocol routers at construction.
#[derive(Clone, Copy, Debug)]
pub struct OperationTarget {
    index: usize,
    operation: &'static OperationSchema<'static>,
    metadata: OperationMetadata,
}
impl OperationTarget {
    pub(super) fn new(index: usize, operation: &'static OperationSchema<'static>) -> Self {
        Self {
            index,
            operation,
            metadata: OperationMetadata::new(operation),
        }
    }

    /// Returns the assigned handler position.
    pub fn index(self) -> usize {
        self.index
    }
    /// Returns the operation's canonical schema.
    pub fn operation(self) -> &'static OperationSchema<'static> {
        self.operation
    }
    /// Returns the input's streaming kind, if any.
    pub fn input_streaming(self) -> Option<StreamingKind> {
        self.metadata.input_streaming
    }
    /// Returns the output's streaming kind, if any.
    pub fn output_streaming(self) -> Option<StreamingKind> {
        self.metadata.output_streaming
    }
    /// Whether the operation consumes a streaming input.
    pub fn has_streaming_input(self) -> bool {
        self.input_streaming().is_some()
    }
    /// Whether the operation produces a streaming output.
    pub fn has_streaming_output(self) -> bool {
        self.output_streaming().is_some()
    }
    /// Whether either the input or output contains a streaming blob.
    pub fn has_streaming_blob(self) -> bool {
        self.input_streaming() == Some(StreamingKind::Blob)
            || self.output_streaming() == Some(StreamingKind::Blob)
    }
}

/// A protocol-independent operation and its HTTP handler, generic over the transport body `B`.
pub struct OperationHandlerBinding<B = hyper::body::Incoming> {
    pub(super) operation: &'static OperationSchema<'static>,
    pub(super) route: SyncRoute<crate::body::RequestBody<B>>,
}
impl<B> OperationHandlerBinding<B> {
    /// Binds an operation to a handler, without assigning any protocol-specific routing rule.
    pub fn new(operation: &'static OperationSchema<'static>, route: SyncRoute<crate::body::RequestBody<B>>) -> Self {
        Self { operation, route }
    }
}
impl<B> Clone for OperationHandlerBinding<B> {
    fn clone(&self) -> Self {
        Self::new(self.operation, self.route.clone())
    }
}
impl<B> fmt::Debug for OperationHandlerBinding<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OperationHandlerBinding")
            .field("operation", &self.operation.shape_id())
            .finish()
    }
}

/// Construction options, independent of any generated router implementation.
#[derive(Clone, Debug, Default)]
pub struct RoutingOptions {
    /// Global and per-operation body-read allowances. Operation entries replace the whole record.
    pub request_body: ServiceRequestBodyConfig,
    /// Per-protocol settings, keyed by protocol shape ID such as
    /// `smithy.protocols#rpcv2Cbor`. Each section is opaque here: the protocol
    /// it names parses it in its `build_router` (see
    /// [`MetadataRoutedProtocol::build_router`](crate::schema::MetadataRoutedProtocol::build_router))
    /// and rejects invalid values with [`RouterBuildError::Configuration`].
    pub protocol_settings: HashMap<String, Document>,
}

/// Everything a protocol sees when building its router: the service, the
/// assigned targets, the server-global configuration, and the protocol's own
/// settings section. Constructed by the routing service, so a protocol never
/// sees another protocol's settings.
#[derive(Debug)]
#[non_exhaustive]
pub struct RouterBuildContext<'a> {
    /// The service schema.
    pub service: &'static ServiceSchema<'static>,
    /// The operations to route, with targets assigned by the routing service.
    pub targets: &'a [OperationTarget],
    /// Server-global body-read allowances. Body-first routing collects under
    /// [`ServiceRequestBodyConfig::for_routing`], enforced by the routing
    /// service itself, not the protocol.
    pub config: &'a ServiceRequestBodyConfig,
    /// This protocol's section of [`RoutingOptions::protocol_settings`], when
    /// one was configured.
    pub protocol_settings: Option<&'a Document>,
}

/// Invalid schema, bindings, or protocol-specific routing configuration.
#[derive(Debug, thiserror::Error)]
pub enum RouterBuildError {
    #[error("no protocol registration recognizes the service schema")]
    UnknownProtocol,
    /// Declared service protocols without a runtime registration, in declaration order.
    #[error("missing protocol registrations: {}", .protocols.join(", "))]
    MissingProtocols { protocols: Vec<String> },
    #[error("invalid operation binding: {0}")]
    Binding(String),
    #[error("invalid routing configuration: {0}")]
    Configuration(String),
    #[error("protocol ordering constraints form a cycle")]
    ProtocolOrderCycle,
    #[error("protocol {protocol} is registered more than once")]
    DuplicateProtocol { protocol: String },
    #[error("no ordering constraint relates protocols {first} and {second}; add a `ProtocolOrder` between them")]
    AmbiguousProtocolOrder { first: String, second: String },
    #[error("protocol could not build its router: {0}")]
    Protocol(#[source] BoxError),
}

/// A protocol's answer to whether a request is its own.
///
/// A multi-protocol service asks its protocols in priority order and dispatches to the first that
/// claims the request. A protocol claims a request only when the request carries every
/// characteristic that identifies the protocol, including naming an operation the service binds.
#[derive(Debug)]
pub enum RouteClaim {
    /// The protocol identifies the request and selects this operation.
    Matched(OperationTarget),
    /// The protocol does not identify the request; the next protocol is asked.
    NoClaim,
    /// The protocol identifies the request but cannot serve it. No other protocol is asked.
    Rejected(RoutingError),
}

/// Selects an operation from request metadata alone.
///
/// The request carries no body: metadata routing never reads one, and keeping the trait
/// body-free keeps it usable behind `dyn` for every transport body type.
///
/// Rejections are the standard [`RoutingError`], classified but not serialized: the routing
/// service hands it to the rejecting protocol's
/// [`serialize_routing_error`](crate::schema::ServerProtocol::serialize_routing_error), which
/// owns the kind-to-wire mapping.
pub trait MetadataProtocolRouter: Send + Sync + fmt::Debug {
    /// Selects from the request URI, method and headers when this is the service's only protocol.
    /// All rejections are terminal.
    fn route(&self, request: &Request<()>) -> Result<OperationTarget, RoutingError>;

    /// Decides whether the request is this protocol's when the service serves several protocols.
    fn claim(&self, request: &Request<()>) -> RouteClaim;

    /// Recognizes a potentially streaming input using only the request head. Output-only
    /// streaming does not qualify. This must perform no body I/O or request-head mutation.
    /// This advisory check delays body claimants; it neither claims nor rejects the request.
    /// The service consults it only when its schema declares a streaming input. Routers
    /// recognize only streaming operations they support; the default recognizes none.
    fn recognizes_streaming_input(&self, _request: &Request<()>) -> bool {
        false
    }
}

/// The router's view of the body bytes the routing service collected for one
/// [`BodyRequirement`].
///
/// This is a view for selection, not the replay artifact: whatever a router reads here, the
/// dispatched handler (or the next claimant) always receives the raw wire bytes, replayed
/// exactly as the transport delivered them.
#[derive(Debug)]
pub struct CollectedBody {
    /// Wire bytes for a plain requirement; the decoder's output for a decoded one.
    pub(super) bytes: Bytes,
    /// Whether `bytes` covers the entire request body.
    pub(super) complete: bool,
}

impl CollectedBody {
    /// The bytes satisfying the requirement: wire bytes for a plain requirement, the
    /// decoder's output for a decoded one.
    pub fn bytes(&self) -> &Bytes {
        &self.bytes
    }

    /// `true` when [`bytes`](Self::bytes) is the entire (decoded) request body; `false` when
    /// it is only the requested prefix.
    pub fn complete(&self) -> bool {
        self.complete
    }
}

/// A protocol-owned codec the routing service runs while satisfying a [`BodyRequirement`].
///
/// The decoder is push-fed by the service's read loop — it can never pull, so it cannot cause
/// over-reading — and each requirement's decoder starts from the first body byte. The codec
/// choice is per-request protocol knowledge (for example, a gzip inflater when the request
/// carries `Content-Encoding: gzip`); the service stays codec-blind.
///
/// A decode error during claiming means the bytes are not this protocol's format: the service
/// treats the claim as [`BodyRouteClaim::NoClaim`] and keeps walking. After
/// [`BodyRouteClaim::MatchedNeedsBody`] the claim is settled, so a decode error is a terminal
/// [`RoutingErrorKind::MalformedRequest`] framed by the matched protocol.
pub trait ClaimDecoder: Send {
    /// Feeds the next raw wire chunk, appending decoded output to `out`.
    fn decode(&mut self, chunk: &[u8], out: &mut Vec<u8>) -> Result<(), BoxError>;
}

/// How much of the request body a [`BodyProtocolRouter`] needs, in decoded bytes.
#[derive(Clone, Copy, Debug)]
pub(super) enum BodyWant {
    Prefix(std::num::NonZeroUsize),
    Complete,
}

/// A body-routed protocol's ask: how much of the body it needs, and through which codec.
///
/// The routing service satisfies the requirement — reading the transport under the service's
/// provisional allowance ([`ServiceRequestBodyConfig::for_routing`], applied to raw wire
/// bytes) — and presents the result as a [`CollectedBody`]. Requirements are self-contained:
/// each decoder is fed the body from its first byte, so no decoder state crosses phases.
pub struct BodyRequirement {
    pub(super) decoder: Option<Box<dyn ClaimDecoder>>,
    pub(super) want: BodyWant,
}

impl BodyRequirement {
    /// The first `n` wire bytes (fewer if the body ends first).
    pub fn prefix(n: std::num::NonZeroUsize) -> Self {
        Self {
            decoder: None,
            want: BodyWant::Prefix(n),
        }
    }

    /// The whole body, as raw wire bytes.
    pub fn complete() -> Self {
        Self {
            decoder: None,
            want: BodyWant::Complete,
        }
    }

    /// The first `n` bytes of `decoder`'s output (fewer if the body ends first).
    pub fn decoded_prefix(decoder: impl ClaimDecoder + 'static, n: std::num::NonZeroUsize) -> Self {
        Self {
            decoder: Some(Box::new(decoder)),
            want: BodyWant::Prefix(n),
        }
    }

    /// The whole body, as `decoder`'s output.
    pub fn decoded_complete(decoder: impl ClaimDecoder + 'static) -> Self {
        Self {
            decoder: Some(Box::new(decoder)),
            want: BodyWant::Complete,
        }
    }
}

impl fmt::Debug for BodyRequirement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BodyRequirement")
            .field("want", &self.want)
            .field("decoded", &self.decoder.is_some())
            .finish()
    }
}

/// A body-routed protocol's answer to whether a request is its own.
///
/// Distinct from [`RouteClaim`] so that needing the body stays unrepresentable for metadata
/// protocols. The two `NeedsBody` variants separate the questions the body answers: whether
/// the request is this protocol's at all, or which operation an already-claimed request names.
#[derive(Debug)]
pub enum BodyRouteClaim {
    /// The protocol identifies the request and selects this operation from the head alone.
    Matched(OperationTarget),
    /// The request is this protocol's — the claim walk ends now — but the operation is named
    /// in the body (a Coral RPC envelope, for example). The service satisfies the requirement
    /// and finishes with [`BodyProtocolRouter::route_with_body`].
    MatchedNeedsBody(BodyRequirement),
    /// The head alone cannot tell whose the request is (a magic-number sniff, for example).
    /// The service satisfies the requirement and asks again with
    /// [`BodyProtocolRouter::claim_with_body`]; the claim stays open, so other protocols may
    /// still be asked.
    ClaimNeedsBody(BodyRequirement),
    /// The protocol does not identify the request; the next protocol is asked.
    NoClaim,
    /// The protocol identifies the request but cannot serve it. No other protocol is asked.
    Rejected(RoutingError),
}

/// Selects an operation for protocols that may read the request body to route.
///
/// Claiming is head-first: [`claim`](Self::claim) sees the request head and escalates to body
/// bytes only by returning a [`BodyRequirement`]. The routing service owns all body I/O — it
/// reads the transport under the service's provisional allowance
/// ([`ServiceRequestBodyConfig::for_routing`]; the selected operation is not known yet, so
/// per-operation allowances cannot apply), buffers every wire byte it reads, and replays them
/// to later claimants and the dispatched handler, so a declined claim never damages the
/// request. The protocol owns interpretation: it may lend the service a [`ClaimDecoder`] to
/// present decoded bytes. Declining from the head whenever possible is not an optimization —
/// it is what keeps a body protocol from stalling requests destined for others on body bytes
/// a client may never send.
///
/// A body-routed protocol serves no streaming operation: the routing service builds its router
/// without them, a body-routed protocol cannot express event-stream framing at all (the
/// [`BodyRoutedProtocol`](crate::schema::BodyRoutedProtocol) subtrait has no such method), and
/// metadata routers can recognize streaming inputs from the head. The service defers body
/// routers for these requests until every metadata router declines. Recognition is advisory:
/// deferred routers may still collect the body during fallback.
///
/// Rejections are the standard [`RoutingError`], exactly as on [`MetadataProtocolRouter`]. When this
/// is the service's only protocol the same claim path runs, with a final
/// [`BodyRouteClaim::NoClaim`] answered as this protocol's
/// [`RoutingError::unknown_operation`] — all rejections stay terminal and protocol-framed.
pub trait BodyProtocolRouter: Send + Sync + fmt::Debug {
    /// Decides from the request head alone, escalating to body bytes via a requirement.
    fn claim(&self, request: &Request<()>) -> BodyRouteClaim;

    /// Continues an open claim over the requested body bytes. Called only after this router
    /// returned [`BodyRouteClaim::ClaimNeedsBody`]; any variant may return, including another
    /// `ClaimNeedsBody` with a larger requirement.
    fn claim_with_body(&self, request: &Request<CollectedBody>) -> BodyRouteClaim {
        let _ = request;
        BodyRouteClaim::NoClaim
    }

    /// Selects the operation a claimed request names in its body. Called only after this
    /// router returned [`BodyRouteClaim::MatchedNeedsBody`]; the claim is settled, so an
    /// unrecognized operation is an error serialized by this protocol, never a fall-through.
    fn route_with_body(&self, request: &Request<CollectedBody>) -> Result<OperationTarget, RoutingError> {
        let _ = request;
        Err(RoutingError::unknown_operation())
    }
}

/// Shared operation router built by a server protocol.
///
/// The variant is decided by the protocol's registration kind: a
/// [`MetadataRoutedProtocol`](crate::schema::MetadataRoutedProtocol) can only build a
/// [`Metadata`](Self::Metadata) router and a
/// [`BodyRoutedProtocol`](crate::schema::BodyRoutedProtocol) a [`Body`](Self::Body) one,
/// so dispatch matching on this enum speaks the claim protocol the registration promised.
#[derive(Clone, Debug)]
pub enum SharedProtocolRouter {
    /// Selects operations from request metadata alone.
    Metadata(Arc<dyn MetadataProtocolRouter>),
    /// May read collected body bytes to select operations.
    Body(Arc<dyn BodyProtocolRouter>),
}

impl SharedProtocolRouter {
    /// Wraps a router that selects from request metadata alone.
    pub fn new(router: impl MetadataProtocolRouter + 'static) -> Self {
        Self::Metadata(Arc::new(router))
    }

    /// Wraps a router that selects from the request body the routing service collects.
    pub fn new_body_routed(router: impl BodyProtocolRouter + 'static) -> Self {
        Self::Body(Arc::new(router))
    }
}
