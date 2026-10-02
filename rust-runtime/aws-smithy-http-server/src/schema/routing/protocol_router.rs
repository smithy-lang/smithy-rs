/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Protocol router interfaces, route claims, and supporting construction types.

use crate::routing::SyncRoute;
use crate::schema::routing::RoutingError;
use crate::schema::{OperationSchema, ServiceSchema};
use crate::{error::BoxError, schema::ServiceRequestBodyConfig};
use aws_smithy_types::Document;
use bytes::Bytes;
use http::Request;

use std::{collections::HashMap, fmt, sync::Arc};

/// The kind of streaming member in an operation's input or output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StreamingKind {
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
            schema.members().iter().find(|member| member.streaming()).map(|member| {
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
    /// Whether the operation consumes a streaming input.
    pub fn has_streaming_input(self) -> bool {
        self.metadata.input_streaming.is_some()
    }
    /// Whether the operation produces a streaming output.
    pub fn has_streaming_output(self) -> bool {
        self.metadata.output_streaming.is_some()
    }
    /// Whether either the input or output contains a streaming blob.
    pub fn has_streaming_blob(self) -> bool {
        self.metadata.input_streaming == Some(StreamingKind::Blob)
            || self.metadata.output_streaming == Some(StreamingKind::Blob)
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
#[non_exhaustive]
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

impl RoutingOptions {
    /// Sets the service body-read allowances.
    pub fn with_request_body(mut self, request_body: ServiceRequestBodyConfig) -> Self {
        self.request_body = request_body;
        self
    }

    /// Sets the per-protocol settings.
    pub fn with_protocol_settings(mut self, protocol_settings: HashMap<String, Document>) -> Self {
        self.protocol_settings = protocol_settings;
        self
    }
}

/// Everything a protocol sees when building its router: the service, the
/// assigned targets, the server-global configuration, and the protocol's own
/// settings section. Constructed by the routing service builder, so a protocol never
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
#[non_exhaustive]
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
    /// Registered protocols participating in ordering cycles, in registration order.
    #[error("protocol ordering constraints form a cycle involving: {}", .protocols.join(", "))]
    ProtocolOrderCycle { protocols: Vec<String> },
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
/// claims the request. Once claimed, routing errors are terminal and framed by that protocol.
#[derive(Debug)]
pub enum RouteClaim {
    /// The protocol claims the request and knows the operation. Dispatch directly.
    ClaimedWithRoute(OperationTarget),
    /// The protocol claims the request. Call [`MetadataProtocolRouter::route`] or
    /// [`BodyProtocolRouter::route_with_body`] to select the operation or return a terminal
    /// routing error. No other protocol is asked.
    Claimed,
    /// The protocol does not identify the request; the next protocol is asked.
    NoClaim,
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
    /// Selects from the request URI, method and headers when this is the service's only
    /// protocol or after [`RouteClaim::Claimed`]. All routing errors are terminal.
    fn route(&self, request: &Request<()>) -> Result<OperationTarget, RoutingError>;

    /// Decides whether the request is this protocol's when the service serves several protocols.
    fn claim(&self, request: &Request<()>) -> RouteClaim;

    /// Recognizes a potentially streaming input using only the request head. Output-only
    /// streaming does not qualify. This must perform no body I/O or request-head mutation.
    /// A match skips routers that need body bytes to claim; it neither claims nor rejects the request.
    /// The service consults it only when its schema declares a streaming input. Routers
    /// recognize only streaming operations they support; the default recognizes none.
    fn recognizes_streaming_input(&self, _request: &Request<()>) -> bool {
        false
    }
}

/// The complete request body collected by the routing service.
///
/// These are the raw wire bytes. The same bytes are replayed to the dispatched handler
/// or the next protocol if the claim is declined.
#[derive(Debug)]
pub struct CollectedBody {
    pub(super) bytes: Bytes,
}

impl CollectedBody {
    /// The complete request body's raw wire bytes.
    pub fn bytes(&self) -> &Bytes {
        &self.bytes
    }
}

/// A body-routed protocol's answer to whether a request is its own.
///
/// Distinct from [`RouteClaim`] so that needing the body stays unrepresentable for metadata
/// protocols. Routing errors are returned by [`BodyProtocolRouter::route_with_body`] after
/// the protocol claims the request.
#[derive(Debug)]
pub enum BodyRouteClaim {
    /// The protocol claims the request and knows the operation. Dispatch directly without
    /// calling [`BodyProtocolRouter::route_with_body`].
    ClaimedWithRoute(OperationTarget),
    /// The protocol claims the request. The service collects the complete body,
    /// then calls [`BodyProtocolRouter::route_with_body`].
    /// No other protocol is asked, including when routing returns an error.
    Claimed,
    /// The protocol needs body bytes to decide whether the request is its own. The service
    /// collects the complete body and calls [`BodyProtocolRouter::claim_with_body`].
    NeedsBodyToClaim,
    /// The request is not this protocol's; the next protocol is asked.
    NoClaim,
}

/// Routes operations using the request head and, when needed, the complete body.
///
/// [`claim`](Self::claim) checks the head first. After `Claimed` or `NeedsBodyToClaim`,
/// the service collects the body under [`ServiceRequestBodyConfig::for_routing`]
/// and preserves it for later routers and the handler. Decline from the head when
/// possible to avoid waiting for a body the client may never send.
///
/// Body routers exclude streaming operations. If any metadata router recognizes
/// streaming input, `NeedsBodyToClaim` is skipped; head claims retain priority.
///
/// Routing errors are terminal and serialized by the claiming protocol.
/// For a single-protocol service, a final `NoClaim` becomes
/// [`RoutingError::unknown_operation`].
pub trait BodyProtocolRouter: Send + Sync + fmt::Debug {
    /// Checks the request head, requesting body collection if needed.
    fn claim(&self, request: &Request<()>) -> BodyRouteClaim;

    /// Continues a [`BodyRouteClaim::NeedsBodyToClaim`] decision with the complete body.
    /// Returning `Claimed` proceeds to [`Self::route_with_body`].
    fn claim_with_body(&self, request: &Request<CollectedBody>) -> RouteClaim {
        let _ = request;
        RouteClaim::NoClaim
    }

    /// Selects an operation after [`BodyRouteClaim::Claimed`] or [`RouteClaim::Claimed`].
    /// Errors are terminal and serialized by this protocol.
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
