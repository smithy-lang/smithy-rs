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
        ProtocolOrder, ProtocolRegistration, ProtocolRegistry, RequestBodyCollectionConfig, SelectedProtocolOperation,
        ServiceRequestBodyConfig, SharedServerProtocol,
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
    route: SyncRoute<crate::body::SchemaBody<B>>,
}
impl<B> OperationHandlerBinding<B> {
    /// Binds an operation to a handler, without assigning any protocol-specific routing rule.
    pub fn new(operation: &'static OperationSchema<'static>, route: SyncRoute<crate::body::SchemaBody<B>>) -> Self {
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
    /// Server-global body-read allowances, for protocols that collect the body
    /// to route (see [`ServiceRequestBodyConfig::for_routing`]).
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

/// The body a body-first router collected while routing.
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

/// The future returned by [`AsyncProtocolRouter::route`].
pub type ProtocolRouteFuture =
    Pin<Box<dyn Future<Output = Result<(OperationIndex, Request<CollectedBody>), Response<BoxBody>>> + Send>>;

/// A body-first protocol's answer to whether a request is its own; see [`RouteClaim`].
#[derive(Debug)]
pub enum AsyncRouteClaim {
    /// The protocol identifies the request and selects this operation.
    Matched(OperationIndex, Request<CollectedBody>),
    /// The protocol does not identify the request. The request comes back with the body the
    /// router collected, and the protocols asked after this one see those bytes.
    NoClaim(Request<CollectedBody>),
    /// The protocol identifies the request but cannot serve it. No other protocol is asked.
    Rejected(Response<BoxBody>),
}

/// The future returned by [`AsyncProtocolRouter::claim`].
pub type ProtocolClaimFuture = Pin<Box<dyn Future<Output = AsyncRouteClaim> + Send>>;

/// Selects an operation for protocols that read the request body to route.
///
/// The router owns the request while routing: it collects the erased body under the allowance
/// it was built with (see [`collect_for_routing`](crate::schema::collect_for_routing)) and
/// returns the request with the [`CollectedBody`]. A body-first protocol always buffers before
/// selecting, so its output is the buffered content, never the transport body — which is what
/// keeps this trait `dyn`-safe and free of the transport body type.
///
/// A body-first protocol serves no streaming operation: the routing service builds its router
/// without them, and rejects a body-first protocol that offers event streams.
pub trait AsyncProtocolRouter: Send + Sync + fmt::Debug {
    /// Selects an operation when this is the service's only protocol, returning the request for
    /// dispatch to its handler. All rejections are terminal and the protocol frames them itself.
    fn route(self: Arc<Self>, request: Request<BoxBody>) -> ProtocolRouteFuture;

    /// Decides whether the request is this protocol's when the service serves several protocols.
    fn claim(self: Arc<Self>, request: Request<BoxBody>) -> ProtocolClaimFuture;
}

/// Shared, erased operation router built by a server protocol.
#[derive(Clone, Debug)]
pub struct SharedProtocolRouter(RouterKind);

#[derive(Clone, Debug)]
enum RouterKind {
    Metadata(Arc<dyn ProtocolRouter>),
    Body(Arc<dyn AsyncProtocolRouter>),
}

impl SharedProtocolRouter {
    /// Wraps a router that selects from request metadata alone.
    pub fn new(router: impl ProtocolRouter + 'static) -> Self {
        Self(RouterKind::Metadata(Arc::new(router)))
    }

    /// Wraps a router that reads the request body to select an operation.
    pub fn new_async(router: impl AsyncProtocolRouter + 'static) -> Self {
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
    route: SyncRoute<crate::body::SchemaBody<B>>,
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
}
impl<B> Clone for Dispatch<B> {
    fn clone(&self) -> Self {
        Self {
            protocols: self.protocols.clone(),
            bindings: self.bindings.clone(),
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
pub struct SchemaRoutingService<B = hyper::body::Incoming> {
    inner: Dispatch<B>,
}
impl<B> Clone for SchemaRoutingService<B> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}
impl<B> fmt::Debug for SchemaRoutingService<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SchemaRoutingService")
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

/// The claim a body-first router started, followed by the claims of every protocol after it.
type ClaimChainFuture =
    Pin<Box<dyn Future<Output = Result<(usize, OperationIndex, Request<CollectedBody>), Response<BoxBody>>> + Send>>;

/// Asks the body-first protocol at `start` and every protocol after it, in priority order.
///
/// Once a body-first router has collected the body, later protocols see the collected bytes: a
/// metadata router probes the head as usual, and a later body-first router reads the bytes again.
fn claim_chain(protocols: Arc<[ProtocolRoute]>, start: usize, request: Request<BoxBody>) -> ClaimChainFuture {
    Box::pin(async move {
        // The transport body, until the first body-first router collects it.
        let mut unread = Some(request);
        // The request with the collected body, once a body-first router has passed on it.
        let mut collected: Option<Request<CollectedBody>> = None;
        for (index, protocol) in protocols.iter().enumerate().skip(start) {
            match &protocol.router.0 {
                RouterKind::Metadata(router) => {
                    let (parts, body) = collected.take().expect("a body-first router went first").into_parts();
                    let probe = Request::from_parts(parts, ());
                    match router.claim(&probe) {
                        RouteClaim::Matched(selected) => {
                            let (parts, ()) = probe.into_parts();
                            return Ok((index, selected, Request::from_parts(parts, body)));
                        }
                        RouteClaim::Rejected(response) => return Err(response),
                        RouteClaim::NoClaim => {
                            let (parts, ()) = probe.into_parts();
                            collected = Some(Request::from_parts(parts, body));
                        }
                    }
                }
                RouterKind::Body(router) => {
                    let request = match collected.take() {
                        Some(request) => request.map(|body| {
                            crate::body::boxed(crate::body::SchemaBody::<BoxBody>::buffered(body.bytes, body.trailers))
                        }),
                        None => unread
                            .take()
                            .expect("the first body-first router reads the transport body"),
                    };
                    match router.clone().claim(request).await {
                        AsyncRouteClaim::Matched(selected, request) => return Ok((index, selected, request)),
                        AsyncRouteClaim::Rejected(response) => return Err(response),
                        AsyncRouteClaim::NoClaim(request) => collected = Some(request),
                    }
                }
            }
        }
        Err(unclaimed())
    })
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
        mut request: Request<crate::body::SchemaBody<B>>,
    ) -> super::route::SyncRouteFuture<crate::body::SchemaBody<B>> {
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

    fn call(&mut self, request: Request<crate::body::SchemaBody<B>>) -> SchemaRoutingFuture<B> {
        let state = if self.protocols.len() == 1 {
            self.route(request)
        } else {
            self.claim(request)
        };
        SchemaRoutingFuture { inner: state }
    }

    /// Routes with the service's only protocol, exactly as a single-protocol service always has.
    fn route(&self, request: Request<crate::body::SchemaBody<B>>) -> State<B> {
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
            // A body-first protocol buffers everything before selecting, so erasing the body
            // here costs one box on a path that allocates the full content anyway.
            RouterKind::Body(router) => State::Routing {
                future: router.clone().route(request.map(crate::body::boxed)),
                dispatch: Some(self.clone()),
            },
        }
    }

    /// Asks each protocol in priority order to claim the request and dispatches to the first that
    /// does. Metadata routers answer synchronously; the first body-first router hands the rest of
    /// the walk to [`claim_chain`].
    fn claim(&self, request: Request<crate::body::SchemaBody<B>>) -> State<B> {
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
                    let request = Request::from_parts(parts, body).map(crate::body::boxed);
                    return State::Claiming {
                        future: claim_chain(self.protocols.clone(), index, request),
                        dispatch: Some(self.clone()),
                    };
                }
            }
        }
        State::Rejected {
            response: Some(unclaimed()),
        }
    }
}

pin_project_lite::pin_project! {
    #[project = StateProj]
    enum State<B> {
        // The routing future owns the request; the dispatch clone shares the handlers. Nothing is
        // borrowed across the await.
        Routing {
            future: ProtocolRouteFuture,
            dispatch: Option<Dispatch<B>>,
        },
        Claiming {
            future: ClaimChainFuture,
            dispatch: Option<Dispatch<B>>,
        },
        Handling {
            #[pin]
            future: super::route::SyncRouteFuture<crate::body::SchemaBody<B>>,
        },
        Rejected {
            response: Option<Response<BoxBody>>,
        },
    }
}

pin_project_lite::pin_project! {
    /// Response future for schema routing.
    pub struct SchemaRoutingFuture<B = hyper::body::Incoming> {
        #[pin]
        inner: State<B>,
    }
}
impl<B> Future for SchemaRoutingFuture<B>
where
    B: http_body::Body<Data = Bytes> + Send + Sync + Unpin + 'static,
    B::Error: Into<BoxError>,
{
    type Output = Result<Response<BoxBody>, Infallible>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        loop {
            match this.inner.as_mut().project() {
                StateProj::Routing { future, dispatch } => match future.as_mut().poll(cx) {
                    Poll::Ready(Ok((selected, request))) => {
                        let dispatch = dispatch.take().expect("routing resolves once");
                        let request = request
                            .map(|collected| crate::body::SchemaBody::buffered(collected.bytes, collected.trailers));
                        let future = dispatch.handle(selected, 0, request);
                        this.inner.set(State::Handling { future });
                    }
                    Poll::Ready(Err(response)) => return Poll::Ready(Ok(response)),
                    Poll::Pending => return Poll::Pending,
                },
                StateProj::Claiming { future, dispatch } => match future.as_mut().poll(cx) {
                    Poll::Ready(Ok((protocol, selected, request))) => {
                        let dispatch = dispatch.take().expect("claiming resolves once");
                        let request = request
                            .map(|collected| crate::body::SchemaBody::buffered(collected.bytes, collected.trailers));
                        let future = dispatch.handle(selected, protocol, request);
                        this.inner.set(State::Handling { future });
                    }
                    Poll::Ready(Err(response)) => return Poll::Ready(Ok(response)),
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

/// Puts the served protocols in priority order.
///
/// Before constraints apply, protocols routing on metadata come before body-first ones, and the
/// built-ins sit in [`BUILTIN_PRIORITY`](crate::schema::protocol::BUILTIN_PRIORITY) order ahead of
/// other protocols, which keep their registration order. [`ProtocolOrder`] constraints then
/// reorder with a stable topological sort; a constraint naming an unserved protocol is ignored.
fn prioritize(
    mut protocols: Vec<(ProtocolRoute, &'static [ProtocolOrder])>,
) -> Result<Vec<ProtocolRoute>, RouterBuildError> {
    let builtin_rank = |route: &ProtocolRoute| {
        crate::schema::protocol::BUILTIN_PRIORITY
            .iter()
            .position(|id| *id == route.protocol.protocol_id().as_str())
            .unwrap_or(usize::MAX)
    };
    protocols.sort_by_key(|(route, _)| (route.router.routes_on_body(), builtin_rank(route)));

    let position = |id: &str| {
        protocols
            .iter()
            .position(|(route, _)| route.protocol.protocol_id().as_str() == id)
    };
    let mut after: Vec<Vec<usize>> = vec![Vec::new(); protocols.len()];
    let mut blockers = vec![0usize; protocols.len()];
    for (index, (_, order)) in protocols.iter().enumerate() {
        for constraint in order.iter() {
            let edge = match *constraint {
                ProtocolOrder::Before(id) => position(id).map(|other| (index, other)),
                ProtocolOrder::After(id) => position(id).map(|other| (other, index)),
            };
            if let Some((first, then)) = edge {
                if first != then && !after[first].contains(&then) {
                    after[first].push(then);
                    blockers[then] += 1;
                }
            }
        }
    }
    let mut placed = vec![false; protocols.len()];
    let mut ordered = Vec::with_capacity(protocols.len());
    while ordered.len() < protocols.len() {
        let next = (0..protocols.len())
            .find(|&index| !placed[index] && blockers[index] == 0)
            .ok_or(RouterBuildError::ProtocolOrderCycle)?;
        placed[next] = true;
        ordered.push(next);
        for &then in &after[next] {
            blockers[then] -= 1;
        }
    }
    let mut protocols: Vec<_> = protocols.into_iter().map(|(route, _)| Some(route)).collect();
    Ok(ordered
        .into_iter()
        .map(|index| protocols[index].take().expect("each protocol is placed once"))
        .collect())
}

impl<B> SchemaRoutingService<B> {
    pub fn from_operation_handler_bindings(
        service: &'static ServiceSchema<'static>,
        registrations: impl IntoIterator<Item = ProtocolRegistration>,
        bindings: impl IntoIterator<Item = OperationHandlerBinding<B>>,
    ) -> Result<Self, RouterBuildError> {
        Self::from_operation_handler_bindings_with_options(service, registrations, bindings, RoutingOptions::default())
    }

    pub fn from_operation_handler_bindings_with_options(
        service: &'static ServiceSchema<'static>,
        registrations: impl IntoIterator<Item = ProtocolRegistration>,
        bindings: impl IntoIterator<Item = OperationHandlerBinding<B>>,
        options: RoutingOptions,
    ) -> Result<Self, RouterBuildError> {
        let mut registry = ProtocolRegistry::builtin();
        for registration in registrations {
            registry = registry.register(registration);
        }
        let resolved = registry.resolve_all(service);
        if resolved.is_empty() {
            return Err(RouterBuildError::UnknownProtocol);
        }
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
        let mut built = Vec::with_capacity(resolved.len());
        for (protocol, order) in resolved {
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
            built.push((ProtocolRoute { router, protocol }, order));
        }
        let protocols = prioritize(built)?;
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
            },
        })
    }

    /// Applies middleware after routing, uniformly to all bound handlers.
    pub fn layer<L>(mut self, layer: &L) -> Self
    where
        B: 'static,
        L: tower::Layer<SyncRoute<crate::body::SchemaBody<B>>>,
        L::Service: Service<Request<crate::body::SchemaBody<B>>, Response = Response<BoxBody>, Error = Infallible>
            + Clone
            + Send
            + Sync
            + 'static,
        <L::Service as Service<Request<crate::body::SchemaBody<B>>>>::Future: Send + 'static,
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
/// [`SchemaBody<B>`](crate::body::SchemaBody) stay unerased; any other body — tests, adapters,
/// upgrade layers — is erased into a boxed state (see [`SchemaBody::new`](crate::body::SchemaBody::new)).
impl<B, RB> Service<Request<RB>> for SchemaRoutingService<B>
where
    B: http_body::Body<Data = Bytes> + Send + Sync + Unpin + 'static,
    B::Error: Into<BoxError>,
    RB: http_body::Body<Data = Bytes> + Send + Sync + 'static,
    RB::Error: Into<BoxError>,
{
    type Response = Response<BoxBody>;
    type Error = Infallible;
    type Future = SchemaRoutingFuture<B>;
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }
    fn call(&mut self, request: Request<RB>) -> Self::Future {
        self.inner.call(request.map(crate::body::SchemaBody::new))
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

/// Builds the awsJson-style target router (`Service.Operation`, honoring
/// [`compat_name`](crate::schema::OperationSchema::compat_name)) for any protocol
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
        let name = target
            .operation
            .compat_name()
            .unwrap_or(target.operation.shape_id().shape_name());
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
