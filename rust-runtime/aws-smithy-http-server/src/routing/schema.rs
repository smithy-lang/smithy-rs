/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Runtime construction and dispatch for the schema protocols a service declares.
//!
//! A service declaring one protocol routes every request with that protocol's [`ProtocolRouter::route`].
//! A service declaring several asks them in priority order to [`claim`](ProtocolRouter::claim) each
//! request and dispatches to the first that does; a request no protocol claims is answered with a
//! bare `400`.

use super::SyncRoute;
use crate::{
    body::BoxBody,
    error::BoxError,
    schema::{
        ProtocolBuildContext, ProtocolOrder, ProtocolRegistration, ProtocolRegistry, RequestBodyCollectionConfig,
        SelectedProtocolOperation, ServiceRequestBodyConfig, SharedServerProtocol,
    },
};
use crate::schema::{OperationSchema, ServiceSchema};
use aws_smithy_types::Document;
use bytes::Bytes;
use http::{Request, Response};

use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    fmt,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tower::Service;

/// A canonical operation schema and its position in the handler array.
#[derive(Clone, Copy, Debug)]
pub struct OperationIndex {
    index: usize,
    operation: &'static OperationSchema<'static>,
}
impl OperationIndex {
    /// Returns the assigned handler position.
    pub fn index(self) -> usize {
        self.index
    }
    /// Returns the operation's canonical schema.
    pub fn operation(self) -> &'static OperationSchema<'static> {
        self.operation
    }
}

/// A protocol-independent operation and its HTTP handler, generic over the transport body `B`.
pub struct OperationHandlerBinding<B = hyper::body::Incoming> {
    operation: &'static OperationSchema<'static>,
    route: SyncRoute<crate::body::RequestBody<B>>,
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
    /// it names parses it in [`ServerProtocol::build_router`] and rejects
    /// invalid values with [`RouterBuildError::Configuration`].
    ///
    /// [`ServerProtocol::build_router`]: crate::schema::ServerProtocol::build_router
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
    pub targets: &'a [OperationIndex],
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
    #[error("invalid operation binding: {0}")]
    Binding(String),
    #[error("invalid routing configuration: {0}")]
    Configuration(String),
    #[error("protocol {protocol} routes on the request body and so cannot support event streams")]
    BodyRoutedEventStream { protocol: String },
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
    Matched(OperationIndex),
    /// The protocol does not identify the request; the next protocol is asked.
    NoClaim,
    /// The protocol identifies the request but cannot serve it. No other protocol is asked.
    Rejected(Response<BoxBody>),
}

/// Selects an operation from request metadata alone.
///
/// The request carries no body: metadata routing never reads one, and keeping the trait
/// body-free keeps it usable behind `dyn` for every transport body type.
pub trait ProtocolRouter: Send + Sync + fmt::Debug {
    /// Selects from the request URI, method and headers when this is the service's only protocol.
    /// All rejections are terminal.
    #[allow(clippy::result_large_err)] // Keep immediate protocol responses allocation-free.
    fn route(&self, request: &Request<()>) -> Result<OperationIndex, Response<BoxBody>>;

    /// Decides whether the request is this protocol's when the service serves several protocols.
    fn claim(&self, request: &Request<()>) -> RouteClaim;
}

/// The body the routing service collected for body-first routing.
///
/// The routing service rebuilds the dispatched request around this content, so the selected
/// handler replays exactly the bytes routing read.
#[derive(Debug)]
pub struct CollectedBody {
    /// The collected content.
    pub bytes: Bytes,
    /// Trailers read with the content, when the transport delivered any.
    pub trailers: Option<http::HeaderMap>,
}

/// Selects an operation for protocols that read the request body to route.
///
/// The routing service owns collection: when it reaches a body-first protocol it buffers the
/// transport body once, under the service's provisional allowance (see
/// [`ServiceRequestBodyConfig::for_routing`]), and presents the request with the
/// [`CollectedBody`]. Selection is therefore synchronous, over already-collected bytes — which
/// keeps this trait `dyn`-safe and free of the transport body type. A collection failure never
/// reaches the router; the service frames it with the first body-first protocol's rejection
/// response. On [`RouteClaim::NoClaim`] the service keeps the collected request, so the
/// protocols asked after this one see the same bytes without reading the transport again.
///
/// A body-first protocol serves no streaming operation: the routing service builds its router
/// without them, and rejects a body-first protocol that offers event streams.
pub trait BodyProtocolRouter: Send + Sync + fmt::Debug {
    /// Selects an operation when this is the service's only protocol. All rejections are
    /// terminal and the protocol frames them itself.
    #[allow(clippy::result_large_err)] // Keep immediate protocol responses allocation-free.
    fn route(&self, request: &Request<CollectedBody>) -> Result<OperationIndex, Response<BoxBody>>;

    /// Decides whether the request is this protocol's when the service serves several protocols.
    fn claim(&self, request: &Request<CollectedBody>) -> RouteClaim;
}

/// Shared, erased operation router built by a server protocol.
#[derive(Clone, Debug)]
pub struct SharedProtocolRouter(RouterKind);

#[derive(Clone, Debug)]
enum RouterKind {
    Metadata(Arc<dyn ProtocolRouter>),
    Body(Arc<dyn BodyProtocolRouter>),
}

impl SharedProtocolRouter {
    /// Wraps a router that selects from request metadata alone.
    pub fn new(router: impl ProtocolRouter + 'static) -> Self {
        Self(RouterKind::Metadata(Arc::new(router)))
    }

    /// Wraps a router that selects from the request body the routing service collects.
    pub fn new_body_routed(router: impl BodyProtocolRouter + 'static) -> Self {
        Self(RouterKind::Body(Arc::new(router)))
    }

    /// Whether operation selection reads the request body.
    pub fn routes_on_body(&self) -> bool {
        matches!(self.0, RouterKind::Body(_))
    }
}

struct BoundHandler<B> {
    operation: &'static OperationSchema<'static>,
    request_body: RequestBodyCollectionConfig,
    route: SyncRoute<crate::body::RequestBody<B>>,
}
impl<B> Clone for BoundHandler<B> {
    fn clone(&self) -> Self {
        Self {
            operation: self.operation,
            request_body: self.request_body,
            route: self.route.clone(),
        }
    }
}
impl<B> fmt::Debug for BoundHandler<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoundHandler")
            .field("operation", &self.operation.shape_id())
            .finish()
    }
}

/// A served protocol and the router it built.
#[derive(Clone, Debug)]
struct ProtocolRoute {
    router: SharedProtocolRouter,
    protocol: SharedServerProtocol,
}

/// Routing state shared by every clone of the service.
///
/// hyper-util's `TowerToHyperService` clones the service for every request, so everything here is
/// behind an `Arc`: a clone is a few reference counts, and dispatch clones only the selected route.
struct Dispatch<B> {
    /// The served protocols in priority order. A single protocol routes with
    /// [`ProtocolRouter::route`]; several claim with [`ProtocolRouter::claim`].
    protocols: Arc<[ProtocolRoute]>,
    bindings: Arc<[BoundHandler<B>]>,
    /// The provisional allowance the service collects under for body-first routing.
    routing_body: RequestBodyCollectionConfig,
}
impl<B> Clone for Dispatch<B> {
    fn clone(&self) -> Self {
        Self {
            protocols: self.protocols.clone(),
            bindings: self.bindings.clone(),
            routing_body: self.routing_body,
        }
    }
}
impl<B> fmt::Debug for Dispatch<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Dispatch")
            .field("protocols", &self.protocols)
            .field("bindings", &self.bindings)
            .finish()
    }
}

/// A service routing normalized requests to a shared handler collection through the protocols the
/// service declares.
///
/// Generic over the transport body `B`: requests entering with the transport's own body flow to
/// handlers unerased. The default is hyper's body; any other request body — tests, upgrade
/// layers, other transports — is accepted and erased into a boxed state on entry.
pub struct MultiProtocolRoutingService<B = hyper::body::Incoming> {
    inner: Dispatch<B>,
}
impl<B> Clone for MultiProtocolRoutingService<B> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}
impl<B> fmt::Debug for MultiProtocolRoutingService<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MultiProtocolRoutingService")
            .field("inner", &self.inner)
            .finish()
    }
}

/// The response for a request no protocol of a multi-protocol service claims.
///
/// No protocol owns the request, so no protocol frames the response: a bare `400` with no body.
fn unclaimed() -> Response<BoxBody> {
    Response::builder()
        .status(http::StatusCode::BAD_REQUEST)
        .body(crate::body::empty())
        .expect("a bare status response is valid")
}

pin_project_lite::pin_project! {
    /// Collects the transport body for body-first routing, under the service's provisional
    /// allowance. The one asynchronous step of body-first routing: everything after it — claim
    /// walk and dispatch — is synchronous, so this future allocates nothing beyond the buffer.
    struct CollectRouting<B> {
        // The concrete pipeline body, unerased: `RequestBody<B>` is `Unpin` for the `B` the
        // service accepts, so frames are polled through `Pin::new`.
        body: crate::body::RequestBody<B>,
        // Zero-copy fast path: a body delivering its content in one frame — a buffered body
        // always does — hands over its `Bytes` without a copy.
        first: Option<Bytes>,
        rest: bytes::BytesMut,
        trailers: Option<http::HeaderMap>,
        config: RequestBodyCollectionConfig,
        // Armed on first poll, so construction needs no runtime context.
        #[pin]
        deadline: Option<tokio::time::Sleep>,
    }
}

impl<B> CollectRouting<B> {
    fn new(body: crate::body::RequestBody<B>, config: RequestBodyCollectionConfig) -> Self {
        Self {
            body,
            first: None,
            rest: bytes::BytesMut::new(),
            trailers: None,
            config,
            deadline: None,
        }
    }
}

impl<B> Future for CollectRouting<B>
where
    B: http_body::Body<Data = Bytes> + Unpin,
    B::Error: Into<BoxError>,
{
    type Output = Result<CollectedBody, crate::schema::RequestBodyCollectionError<crate::Error>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        use crate::schema::RequestBodyCollectionError;
        use http_body::Body as _;
        let mut this = self.project();
        if let Some(timeout) = this.config.read_timeout {
            if this.deadline.is_none() {
                this.deadline.set(Some(tokio::time::sleep(timeout)));
            }
            let deadline = this.deadline.as_mut().as_pin_mut().expect("armed above");
            if deadline.poll(cx).is_ready() {
                return Poll::Ready(Err(RequestBodyCollectionError::Timeout { timeout }));
            }
        }
        loop {
            match Pin::new(&mut *this.body).poll_frame(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => {
                    let bytes = this.first.take().unwrap_or_else(|| std::mem::take(this.rest).freeze());
                    return Poll::Ready(Ok(CollectedBody {
                        bytes,
                        trailers: this.trailers.take(),
                    }));
                }
                Poll::Ready(Some(Err(err))) => return Poll::Ready(Err(RequestBodyCollectionError::Body(err))),
                Poll::Ready(Some(Ok(frame))) => match frame.into_data() {
                    Ok(data) => {
                        let read = this.first.as_ref().map_or(0, Bytes::len) + this.rest.len();
                        if let Some(limit) = this.config.max_bytes {
                            if data.len() > limit.get().saturating_sub(read) {
                                return Poll::Ready(Err(RequestBodyCollectionError::TooLarge(
                                    crate::body::BodyLimitExceeded { limit: limit.get() },
                                )));
                            }
                        }
                        if read == 0 {
                            *this.first = Some(data);
                        } else {
                            if let Some(first) = this.first.take() {
                                this.rest.extend_from_slice(&first);
                            }
                            this.rest.extend_from_slice(&data);
                        }
                    }
                    Err(frame) => {
                        if let Ok(new_trailers) = frame.into_trailers() {
                            this.trailers.get_or_insert_with(http::HeaderMap::new).extend(new_trailers);
                        }
                    }
                },
            }
        }
    }
}

impl<B> Dispatch<B>
where
    B: http_body::Body<Data = Bytes> + Send + Sync + Unpin + 'static,
    B::Error: Into<BoxError>,
{
    /// Hands the routed request to its handler, recording the selection for downstream consumers.
    fn handle(
        &self,
        selected: OperationIndex,
        protocol: usize,
        mut request: Request<crate::body::RequestBody<B>>,
    ) -> super::route::SyncRouteFuture<crate::body::RequestBody<B>> {
        let binding = &self.bindings[selected.index];
        debug_assert!(
            std::ptr::eq(binding.operation, selected.operation),
            "router index belongs to a different operation"
        );
        request.extensions_mut().insert(SelectedProtocolOperation::new(
            self.protocols[protocol].protocol.clone(),
            binding.operation,
            binding.request_body,
        ));
        binding.route.clone().call_owned(request)
    }

    fn call(&mut self, request: Request<crate::body::RequestBody<B>>) -> MultiProtocolRoutingFuture<B> {
        let state = if self.protocols.len() == 1 {
            self.route(request)
        } else {
            self.claim(request)
        };
        MultiProtocolRoutingFuture { inner: state }
    }

    /// Routes with the service's only protocol, exactly as a single-protocol service always has.
    fn route(&self, request: Request<crate::body::RequestBody<B>>) -> State<B> {
        match &self.protocols[0].router.0 {
            RouterKind::Metadata(router) => {
                // Probe with the head only: the parts move over and back, nothing is cloned,
                // and the router stays free of the transport body type.
                let (parts, body) = request.into_parts();
                let probe = Request::from_parts(parts, ());
                match router.route(&probe) {
                    Ok(selected) => {
                        let (parts, ()) = probe.into_parts();
                        State::Handling {
                            future: self.handle(selected, 0, Request::from_parts(parts, body)),
                        }
                    }
                    Err(response) => State::Rejected {
                        response: Some(response),
                    },
                }
            }
            // A body-first protocol selects from collected bytes; the service collects first.
            RouterKind::Body(_) => self.collect(request, 0),
        }
    }

    /// Asks each protocol in priority order to claim the request and dispatches to the first that
    /// does. Metadata routers answer from the head; the first body-first router suspends the walk
    /// while the service collects the body, and [`Self::routed`] finishes it synchronously.
    fn claim(&self, request: Request<crate::body::RequestBody<B>>) -> State<B> {
        let (parts, body) = request.into_parts();
        let probe = Request::from_parts(parts, ());
        for (index, protocol) in self.protocols.iter().enumerate() {
            match &protocol.router.0 {
                RouterKind::Metadata(router) => match router.claim(&probe) {
                    RouteClaim::Matched(selected) => {
                        let (parts, ()) = probe.into_parts();
                        return State::Handling {
                            future: self.handle(selected, index, Request::from_parts(parts, body)),
                        };
                    }
                    RouteClaim::Rejected(response) => {
                        return State::Rejected {
                            response: Some(response),
                        }
                    }
                    RouteClaim::NoClaim => {}
                },
                RouterKind::Body(_) => {
                    let (parts, ()) = probe.into_parts();
                    return self.collect(Request::from_parts(parts, body), index);
                }
            }
        }
        State::Rejected {
            response: Some(unclaimed()),
        }
    }

    /// Suspends routing while the body is collected for the body-first protocol at `start`.
    fn collect(&self, request: Request<crate::body::RequestBody<B>>, start: usize) -> State<B> {
        let (parts, body) = request.into_parts();
        State::Collecting {
            parts: Some(parts),
            collect: CollectRouting::new(body, self.routing_body),
            start,
            dispatch: Some(self.clone()),
        }
    }

    /// Finishes routing over the collected body, synchronously.
    ///
    /// A single-protocol service routes with the body-first protocol's terminal
    /// [`BodyProtocolRouter::route`]. A multi-protocol service resumes the claim walk at `start`:
    /// a metadata router probes the head as usual, and every body-first router sees the same
    /// collected bytes — the transport is never read again.
    fn routed(&self, parts: http::request::Parts, collected: CollectedBody, start: usize) -> State<B> {
        let mut request = Request::from_parts(parts, collected);
        if self.protocols.len() == 1 {
            let RouterKind::Body(router) = &self.protocols[0].router.0 else {
                unreachable!("only a body-first protocol suspends single-protocol routing");
            };
            return match router.route(&request) {
                Ok(selected) => self.dispatch_collected(selected, 0, request),
                Err(response) => State::Rejected {
                    response: Some(response),
                },
            };
        }
        for (index, protocol) in self.protocols.iter().enumerate().skip(start) {
            let claim = match &protocol.router.0 {
                RouterKind::Metadata(router) => {
                    let (parts, body) = request.into_parts();
                    let probe = Request::from_parts(parts, ());
                    let claim = router.claim(&probe);
                    let (parts, ()) = probe.into_parts();
                    request = Request::from_parts(parts, body);
                    claim
                }
                RouterKind::Body(router) => router.claim(&request),
            };
            match claim {
                RouteClaim::Matched(selected) => return self.dispatch_collected(selected, index, request),
                RouteClaim::Rejected(response) => {
                    return State::Rejected {
                        response: Some(response),
                    }
                }
                RouteClaim::NoClaim => {}
            }
        }
        State::Rejected {
            response: Some(unclaimed()),
        }
    }

    /// Hands a routed request to its handler with the collected body, replayed as buffered content.
    fn dispatch_collected(
        &self,
        selected: OperationIndex,
        protocol: usize,
        request: Request<CollectedBody>,
    ) -> State<B> {
        let (parts, collected) = request.into_parts();
        let body = crate::body::RequestBody::buffered(collected.bytes, collected.trailers);
        State::Handling {
            future: self.handle(selected, protocol, Request::from_parts(parts, body)),
        }
    }
}

pin_project_lite::pin_project! {
    #[project = StateProj]
    enum State<B> {
        // Body-first routing, suspended on collection. The state owns the request head and the
        // collection future; the dispatch clone shares the handlers. Nothing is borrowed across
        // the suspension, and nothing here is boxed.
        Collecting {
            parts: Option<http::request::Parts>,
            #[pin]
            collect: CollectRouting<B>,
            start: usize,
            dispatch: Option<Dispatch<B>>,
        },
        Handling {
            #[pin]
            future: super::route::SyncRouteFuture<crate::body::RequestBody<B>>,
        },
        Rejected {
            response: Option<Response<BoxBody>>,
        },
    }
}

pin_project_lite::pin_project! {
    /// Response future for schema routing.
    pub struct MultiProtocolRoutingFuture<B = hyper::body::Incoming> {
        #[pin]
        inner: State<B>,
    }
}
impl<B> Future for MultiProtocolRoutingFuture<B>
where
    B: http_body::Body<Data = Bytes> + Send + Sync + Unpin + 'static,
    B::Error: Into<BoxError>,
{
    type Output = Result<Response<BoxBody>, Infallible>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        loop {
            match this.inner.as_mut().project() {
                StateProj::Collecting {
                    parts,
                    collect,
                    start,
                    dispatch,
                } => match collect.poll(cx) {
                    Poll::Ready(Ok(collected)) => {
                        let dispatch = dispatch.take().expect("collection resolves once");
                        let parts = parts.take().expect("collection resolves once");
                        let start = *start;
                        this.inner.set(dispatch.routed(parts, collected, start));
                    }
                    // A collection failure precedes any claim, so the first body-first
                    // protocol — the one collection was for — frames the rejection, exactly
                    // as when it collected for itself.
                    Poll::Ready(Err(error)) => {
                        let dispatch = dispatch.take().expect("collection resolves once");
                        let protocol = &dispatch.protocols[*start].protocol;
                        return Poll::Ready(Ok(crate::schema::body_collection_rejection(&**protocol, error)));
                    }
                    Poll::Pending => return Poll::Pending,
                },
                StateProj::Handling { future } => return future.poll(cx),
                StateProj::Rejected { response } => {
                    return Poll::Ready(Ok(response.take().expect("polled after completion")))
                }
            }
        }
    }
}

fn has_streaming_member(operation: &OperationSchema<'_>) -> bool {
    [operation.input(), operation.output()]
        .iter()
        .any(|schema| schema.members().iter().any(|member| member.streaming()))
}

/// Resolves the registered protocols the service declares, in claim order.
///
/// The order comes from [`ProtocolOrder`] constraints alone, resolved over the **global** set of
/// registered protocols: a constraint against a protocol the service does not serve still orders
/// the ones it does, transitively. Registry and declaration order carry no meaning. Errors:
/// a protocol registered twice, a constraint naming an unregistered protocol, a constraint
/// cycle, or two served protocols the constraints leave unordered.
fn resolve_protocols(
    service: &'static ServiceSchema<'static>,
    registries: impl IntoIterator<Item = &'static ProtocolRegistry>,
    options: &RoutingOptions,
) -> Result<Vec<SharedServerProtocol>, RouterBuildError> {
    let mut registrations: Vec<ProtocolRegistration> = Vec::new();
    registrations.extend_from_slice(ProtocolRegistry::BUILTIN.registrations());
    for registry in registries {
        registrations.extend_from_slice(registry.registrations());
    }
    for (index, registration) in registrations.iter().enumerate() {
        if registrations[..index]
            .iter()
            .any(|other| other.protocol_id() == registration.protocol_id())
        {
            return Err(RouterBuildError::DuplicateProtocol {
                protocol: registration.protocol_id().to_string(),
            });
        }
    }

    // Reachability over the global constraint graph, absent protocols included as transit nodes.
    let count = registrations.len();
    let position = |id: &str| registrations.iter().position(|registration| registration.protocol_id() == id);
    let mut reaches = vec![vec![false; count]; count];
    for (index, registration) in registrations.iter().enumerate() {
        for constraint in registration.order() {
            let id = match *constraint {
                ProtocolOrder::Before(id) | ProtocolOrder::After(id) => id,
            };
            let other = position(id).ok_or_else(|| {
                RouterBuildError::Configuration(format!(
                    "protocol {} orders against unregistered protocol {id}",
                    registration.protocol_id()
                ))
            })?;
            match *constraint {
                ProtocolOrder::Before(_) => reaches[index][other] = true,
                ProtocolOrder::After(_) => reaches[other][index] = true,
            }
        }
    }
    for via in 0..count {
        for from in 0..count {
            if reaches[from][via] {
                for to in 0..count {
                    if reaches[via][to] {
                        reaches[from][to] = true;
                    }
                }
            }
        }
    }
    if (0..count).any(|index| reaches[index][index]) {
        return Err(RouterBuildError::ProtocolOrderCycle);
    }

    let mut served: Vec<usize> = registrations
        .iter()
        .enumerate()
        .filter(|(_, registration)| {
            service
                .protocols()
                .iter()
                .any(|protocol| protocol.as_str() == registration.protocol_id())
        })
        .map(|(index, _)| index)
        .collect();
    if served.is_empty() {
        return Err(RouterBuildError::UnknownProtocol);
    }
    for (nth, &first) in served.iter().enumerate() {
        for &second in &served[nth + 1..] {
            if !reaches[first][second] && !reaches[second][first] {
                return Err(RouterBuildError::AmbiguousProtocolOrder {
                    first: registrations[first].protocol_id().to_string(),
                    second: registrations[second].protocol_id().to_string(),
                });
            }
        }
    }
    // Every served pair is comparable and the graph is acyclic, so reachability totally orders them.
    served.sort_by(|&first, &second| {
        if reaches[first][second] {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }
    });

    served
        .into_iter()
        .map(|index| {
            let registration = &registrations[index];
            let context = ProtocolBuildContext::new(service)
                .with_settings(options.protocol_settings.get(registration.protocol_id()))
                .with_global(options.protocol_settings.get("global"));
            registration.build(&context)
        })
        .collect()
}

impl<B> MultiProtocolRoutingService<B> {
    pub fn from_operation_handler_bindings(
        service: &'static ServiceSchema<'static>,
        registries: impl IntoIterator<Item = &'static ProtocolRegistry>,
        bindings: impl IntoIterator<Item = OperationHandlerBinding<B>>,
    ) -> Result<Self, RouterBuildError> {
        Self::from_operation_handler_bindings_with_options(service, registries, bindings, RoutingOptions::default())
    }

    /// Builds the routing service. [`ProtocolRegistry::BUILTIN`] is always consulted; `registries`
    /// contribute protocols implemented outside this crate.
    pub fn from_operation_handler_bindings_with_options(
        service: &'static ServiceSchema<'static>,
        registries: impl IntoIterator<Item = &'static ProtocolRegistry>,
        bindings: impl IntoIterator<Item = OperationHandlerBinding<B>>,
        options: RoutingOptions,
    ) -> Result<Self, RouterBuildError> {
        let resolved = resolve_protocols(service, registries, &options)?;
        let bindings: Vec<_> = bindings.into_iter().collect();
        let mut seen = HashSet::new();
        for binding in &bindings {
            let id = binding.operation.shape_id().as_str();
            if !seen.insert(id)
                || !service
                    .operations()
                    .iter()
                    .any(|op| std::ptr::eq(*op, binding.operation))
            {
                return Err(RouterBuildError::Binding(id.to_owned()));
            }
        }
        for operation in service.operations() {
            if !seen.contains(operation.shape_id().as_str()) {
                return Err(RouterBuildError::Binding(format!("missing {}", operation.shape_id())));
            }
        }
        if seen.len() != service.operations().len() {
            return Err(RouterBuildError::Binding("duplicate service operation schemas".into()));
        }
        for id in options.request_body.per_operation.keys() {
            if !seen.contains(id.as_str()) {
                return Err(RouterBuildError::Configuration(format!("unknown operation {id}")));
            }
        }
        let targets: Vec<_> = bindings
            .iter()
            .enumerate()
            .map(|(index, binding)| OperationIndex {
                index,
                operation: binding.operation,
            })
            .collect();
        // A body-first router collects the body before it selects, so it never sees a streaming
        // operation: those requests are not its to claim.
        let non_streaming: Vec<_> = targets
            .iter()
            .filter(|target| !has_streaming_member(target.operation))
            .copied()
            .collect();
        // `resolved` is already in claim order; build each protocol's router in place.
        let mut protocols = Vec::with_capacity(resolved.len());
        for protocol in resolved {
            let context = |targets| RouterBuildContext {
                service,
                targets,
                config: &options.request_body,
                protocol_settings: options.protocol_settings.get(protocol.protocol_id().as_str()),
            };
            let mut router = protocol.build_router(context(&targets))?;
            if router.routes_on_body() {
                if protocol.event_stream().is_some() {
                    return Err(RouterBuildError::BodyRoutedEventStream {
                        protocol: protocol.protocol_id().to_string(),
                    });
                }
                if non_streaming.len() != targets.len() {
                    router = protocol.build_router(context(&non_streaming))?;
                }
            }
            protocols.push(ProtocolRoute { router, protocol });
        }
        let bindings = bindings
            .into_iter()
            .map(|binding| BoundHandler {
                operation: binding.operation,
                request_body: options.request_body.for_operation(binding.operation.shape_id()),
                route: binding.route,
            })
            .collect();
        Ok(Self {
            inner: Dispatch {
                protocols: protocols.into(),
                bindings,
                routing_body: options.request_body.for_routing(),
            },
        })
    }

    /// Applies middleware after routing, uniformly to all bound handlers.
    pub fn layer<L>(mut self, layer: &L) -> Self
    where
        B: 'static,
        L: tower::Layer<SyncRoute<crate::body::RequestBody<B>>>,
        L::Service: Service<Request<crate::body::RequestBody<B>>, Response = Response<BoxBody>, Error = Infallible>
            + Clone
            + Send
            + Sync
            + 'static,
        <L::Service as Service<Request<crate::body::RequestBody<B>>>>::Future: Send + 'static,
    {
        self.inner.bindings = self
            .inner
            .bindings
            .iter()
            .cloned()
            .map(|binding| BoundHandler {
                operation: binding.operation,
                request_body: binding.request_body,
                route: SyncRoute::new(layer.layer(binding.route)),
            })
            .collect();
        self
    }
}
/// Any compatible body enters. The transport body `B` and an already-normalized
/// [`RequestBody<B>`](crate::body::RequestBody) stay unerased; any other body — tests, adapters,
/// upgrade layers — is erased into a boxed state (see [`RequestBody::new`](crate::body::RequestBody::new)).
impl<B, RB> Service<Request<RB>> for MultiProtocolRoutingService<B>
where
    B: http_body::Body<Data = Bytes> + Send + Sync + Unpin + 'static,
    B::Error: Into<BoxError>,
    RB: http_body::Body<Data = Bytes> + Send + Sync + 'static,
    RB::Error: Into<BoxError>,
{
    type Response = Response<BoxBody>;
    type Error = Infallible;
    type Future = MultiProtocolRoutingFuture<B>;
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }
    fn call(&mut self, request: Request<RB>) -> Self::Future {
        self.inner.call(request.map(crate::body::RequestBody::new))
    }
}

/// Whether the request's `Content-Type` names `expected`, ignoring parameters.
fn content_type_is(request: &Request<()>, expected: &str) -> bool {
    request
        .headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<mime::Mime>().ok())
        .is_some_and(|mime| mime.essence_str() == expected)
}

/// Whether the request head announces an empty body.
fn announces_no_body(request: &Request<()>) -> bool {
    let headers = request.headers();
    !headers.contains_key(http::header::TRANSFER_ENCODING)
        && headers
            .get(http::header::CONTENT_LENGTH)
            .is_none_or(|length| length.as_bytes() == b"0")
}

/// The `Content-Type` a REST protocol requires to claim a request for one operation.
#[derive(Debug)]
enum ClaimContentType {
    /// The header does not tell the protocol apart; method and path decide.
    Any,
    /// The header must be absent.
    Absent,
    /// The header must name this media type, or be absent with an empty body.
    Expect(mime::Mime),
}

impl ClaimContentType {
    fn for_input(input: &aws_smithy_schema::Schema<'_>, codec_content_type: &'static str) -> Self {
        use crate::schema::protocol::request::{expected_request_content_type, ExpectedContentType};
        let custom = input.members().iter().any(|member| {
            member
                .http_header()
                .is_some_and(|header| header.value().eq_ignore_ascii_case("content-type"))
        });
        if custom {
            return Self::Any;
        }
        match expected_request_content_type(input, codec_content_type) {
            ExpectedContentType::Skip => Self::Any,
            ExpectedContentType::Absent => Self::Absent,
            ExpectedContentType::Expect(mime) => Self::Expect(mime),
        }
    }

    fn admits(&self, request: &Request<()>) -> bool {
        let present = request.headers().contains_key(http::header::CONTENT_TYPE);
        match self {
            Self::Any => true,
            Self::Absent => !present,
            Self::Expect(mime) if present => content_type_is(request, mime.essence_str()),
            Self::Expect(_) => announces_no_body(request),
        }
    }
}

/// Routes restJson1 and restXml on each operation's `@http` method and URI.
///
/// Claims a request whose method and path match an operation and whose `Content-Type` matches the
/// one that operation's input derives: the protocol's media type when members are bound to the
/// body, the payload's for an `@httpPayload`, none for an input without members. An input binding
/// `Content-Type` with `@httpHeader`, or one whose content type the header cannot distinguish (a
/// streaming or untyped blob payload, members bound only to the URI and headers), is claimed on
/// method and path alone; so is a request with neither `Content-Type` nor a body.
///
/// A service serving both restJson1 and restXml therefore cannot tell them apart for such requests:
/// the protocol earlier in priority order, restJson1 unless reordered, claims them, and a client of
/// the other protocol receives a response it cannot read. Requests with a structured body carry
/// `application/json` or `application/xml` and reach the right protocol.
#[derive(Debug)]
struct RestProtocolRouter<P> {
    router: crate::protocol::rest::router::RestRouter<OperationIndex>,
    /// Indexed by [`OperationIndex::index`].
    content_types: Vec<ClaimContentType>,
    protocol: std::marker::PhantomData<fn() -> P>,
}
impl<P> ProtocolRouter for RestProtocolRouter<P>
where
    crate::protocol::rest::router::Error: crate::response::IntoResponse<P>,
    P: fmt::Debug,
{
    fn route(&self, request: &Request<()>) -> Result<OperationIndex, Response<BoxBody>> {
        use super::Router;
        self.router
            .match_route(request)
            .map_err(crate::response::IntoResponse::<P>::into_response)
    }

    fn claim(&self, request: &Request<()>) -> RouteClaim {
        use super::Router;
        match self.router.match_route(request) {
            Ok(target) if self.content_types[target.index].admits(request) => RouteClaim::Matched(target),
            Ok(_) | Err(_) => RouteClaim::NoClaim,
        }
    }
}

/// Routes awsJson1.0 and awsJson1.1 on `X-Amz-Target`.
///
/// Claims a `POST` to the path `/` whose `Content-Type` is the protocol's media type and whose
/// `X-Amz-Target` names an operation the service binds.
#[derive(Debug)]
struct AwsJsonProtocolRouter<P> {
    router: crate::protocol::aws_json::router::AwsJsonRouter<OperationIndex>,
    content_type: &'static str,
    protocol: std::marker::PhantomData<fn() -> P>,
}
impl<P> ProtocolRouter for AwsJsonProtocolRouter<P>
where
    crate::protocol::aws_json::router::Error: crate::response::IntoResponse<P>,
    P: fmt::Debug,
{
    fn route(&self, request: &Request<()>) -> Result<OperationIndex, Response<BoxBody>> {
        use super::Router;
        self.router
            .match_route(request)
            .map_err(crate::response::IntoResponse::<P>::into_response)
    }

    fn claim(&self, request: &Request<()>) -> RouteClaim {
        // The path, not the whole URI: clients may add query parameters awsJson ignores.
        if request.method() != http::Method::POST
            || request.uri().path() != "/"
            || !content_type_is(request, self.content_type)
        {
            return RouteClaim::NoClaim;
        }
        match self.router.match_target(request) {
            Some(target) => RouteClaim::Matched(target),
            None => RouteClaim::NoClaim,
        }
    }
}

/// Routes rpcv2Cbor on the `/service/{service}/operation/{operation}` path.
///
/// Claims a `POST` carrying `Smithy-Protocol: rpc-v2-cbor` whose path names an operation the
/// service binds. The protocol does not stream blobs: a claimed operation with a streaming blob
/// member is rejected as an unknown operation, as is a request carrying a header the protocol
/// forbids.
#[derive(Debug)]
struct RpcV2CborProtocolRouter {
    router: crate::protocol::rpc_v2_cbor::router::RpcV2CborRouter<OperationIndex>,
    /// Indexed by [`OperationIndex::index`].
    streams_blobs: Vec<bool>,
}
impl RpcV2CborProtocolRouter {
    fn unsupported(&self, target: OperationIndex) -> bool {
        self.streams_blobs[target.index]
    }

    fn reject(error: crate::protocol::rpc_v2_cbor::router::Error) -> Response<BoxBody> {
        crate::response::IntoResponse::<crate::protocol::rpc_v2_cbor::RpcV2Cbor>::into_response(error)
    }
}
impl ProtocolRouter for RpcV2CborProtocolRouter {
    fn route(&self, request: &Request<()>) -> Result<OperationIndex, Response<BoxBody>> {
        use super::Router;
        use crate::protocol::rpc_v2_cbor::router::Error;
        match self.router.match_route(request) {
            Ok(target) if self.unsupported(target) => Err(Self::reject(Error::NotFound)),
            Ok(target) => Ok(target),
            Err(error) => Err(Self::reject(error)),
        }
    }

    fn claim(&self, request: &Request<()>) -> RouteClaim {
        use super::Router;
        use crate::protocol::rpc_v2_cbor::router::Error;
        let identified = request.method() == http::Method::POST
            && request
                .headers()
                .get("smithy-protocol")
                .is_some_and(|value| value.as_bytes() == b"rpc-v2-cbor");
        if !identified {
            return RouteClaim::NoClaim;
        }
        match self.router.match_route(request) {
            Ok(target) if self.unsupported(target) => RouteClaim::Rejected(Self::reject(Error::NotFound)),
            Ok(target) => RouteClaim::Matched(target),
            Err(Error::ForbiddenHeaders) => RouteClaim::Rejected(Self::reject(Error::ForbiddenHeaders)),
            Err(_) => RouteClaim::NoClaim,
        }
    }
}

/// Sizes a per-operation table to cover every target index.
fn per_target<T>(
    targets: &[OperationIndex],
    default: impl Fn() -> T,
    mut value: impl FnMut(OperationIndex) -> T,
) -> Vec<T> {
    let len = targets.iter().map(|target| target.index + 1).max().unwrap_or(0);
    let mut table: Vec<T> = (0..len).map(|_| default()).collect();
    for target in targets {
        table[target.index] = value(*target);
    }
    table
}

pub(crate) fn rest_router<P: fmt::Debug + 'static>(
    targets: &[OperationIndex],
    codec_content_type: &'static str,
) -> Result<SharedProtocolRouter, RouterBuildError>
where
    crate::protocol::rest::router::Error: crate::response::IntoResponse<P>,
{
    use super::request_spec::{PathSegment, QuerySegment, RequestSpec};
    let entries = targets
        .iter()
        .map(|target| {
            let http = target.operation.http().ok_or_else(|| {
                RouterBuildError::Configuration(format!("missing HTTP trait on {}", target.operation.shape_id()))
            })?;
            let method = http
                .method()
                .parse()
                .map_err(|err| RouterBuildError::Protocol(Box::new(err)))?;
            let (path, query) = http.uri().split_once('?').unwrap_or((http.uri(), ""));
            let segments = path
                .trim_start_matches('/')
                .split('/')
                .filter(|s| !s.is_empty())
                .map(|segment| {
                    if segment.starts_with('{') && segment.ends_with("+}") {
                        PathSegment::Greedy
                    } else if segment.starts_with('{') && segment.ends_with('}') {
                        PathSegment::Label
                    } else {
                        PathSegment::Literal(segment.to_owned())
                    }
                })
                .collect();
            let query = form_urlencoded::parse(query.as_bytes())
                .map(|(key, value)| {
                    if value.is_empty() {
                        QuerySegment::Key(key.into_owned())
                    } else {
                        QuerySegment::KeyValue(key.into_owned(), value.into_owned())
                    }
                })
                .collect();
            Ok((
                RequestSpec::new(
                    method,
                    super::request_spec::UriSpec::new(super::request_spec::PathAndQuerySpec::new(
                        super::request_spec::PathSpec::from_vector_unchecked(segments),
                        super::request_spec::QuerySpec::from_vector_unchecked(query),
                    )),
                ),
                *target,
            ))
        })
        .collect::<Result<Vec<_>, RouterBuildError>>()?;
    let content_types = per_target(
        targets,
        || ClaimContentType::Any,
        |target| ClaimContentType::for_input(target.operation.input(), codec_content_type),
    );
    Ok(SharedProtocolRouter::new(RestProtocolRouter::<P> {
        router: crate::protocol::rest::router::RestRouter::from_iter(entries),
        content_types,
        protocol: std::marker::PhantomData,
    }))
}

/// Builds the awsJson-style target router (`Service.Operation`) for any protocol
/// marker `P` whose rejections convert like awsJson's. Exposed for out-of-tree protocols that
/// route on the same key. Among several protocols, the router claims `POST /` requests whose
/// `Content-Type` is `content_type`.
#[doc(hidden)]
pub fn aws_json_router<P: fmt::Debug + 'static>(
    ctx: &RouterBuildContext<'_>,
    content_type: &'static str,
) -> Result<SharedProtocolRouter, RouterBuildError>
where
    crate::protocol::aws_json::router::Error: crate::response::IntoResponse<P>,
{
    let entries = ctx.targets.iter().map(|target| {
        let name = target.operation.shape_id().shape_name();
        (format!("{}.{}", ctx.service.shape_id().shape_name(), name), *target)
    });
    Ok(SharedProtocolRouter::new(AwsJsonProtocolRouter::<P> {
        router: crate::protocol::aws_json::router::AwsJsonRouter::from_owned(entries),
        content_type,
        protocol: std::marker::PhantomData,
    }))
}

pub(crate) fn rpc_v2_cbor_router(ctx: &RouterBuildContext<'_>) -> Result<SharedProtocolRouter, RouterBuildError> {
    let capitalize_routes = crate::schema::protocol::settings_bool(ctx.protocol_settings, "capitalizeRoutes")?;
    let entries = ctx.targets.iter().flat_map(|target| {
        let name = target.operation.shape_id().shape_name();
        let mut names = vec![name.to_owned()];
        if capitalize_routes {
            let mut chars = name.chars();
            if let Some(first) = chars.next() {
                let alias = format!("{}{}", first.to_uppercase(), chars.as_str());
                if alias != name {
                    names.push(alias);
                }
            }
        }
        names
            .into_iter()
            .map(move |name| (format!("{}.{}", ctx.service.shape_id().shape_name(), name), *target))
    });
    let streams_blobs = per_target(
        ctx.targets,
        || false,
        |target| {
            [target.operation.input(), target.operation.output()]
                .iter()
                .any(|schema| {
                    schema
                        .members()
                        .iter()
                        .any(|member| member.streaming() && member.shape_type() == aws_smithy_schema::ShapeType::Blob)
                })
        },
    );
    Ok(SharedProtocolRouter::new(RpcV2CborProtocolRouter {
        router: crate::protocol::rpc_v2_cbor::router::RpcV2CborRouter::from_owned(entries),
        streams_blobs,
    }))
}

#[cfg(test)]
mod tests;
