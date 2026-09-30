/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The multi-protocol routing service: dispatch state, the claim walk, and its futures.

use crate::routing::SyncRoute;
use crate::{
    body::BoxBody,
    error::BoxError,
    schema::{
        ProtocolBuildContext, ProtocolOrder, ProtocolRegistration, ProtocolRegistry, RequestBodyCollectionConfig,
        SelectedProtocolOperation, SharedServerProtocol,
    },
};
use crate::schema::{OperationSchema, ServiceSchema};
use crate::schema::routing::RoutingError;
use bytes::Bytes;
use http::{Request, Response};

use std::{
    collections::HashSet,
    convert::Infallible,
    fmt,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tower::Service;
use super::collect::BodyCollector;
use super::contract::BodyWant;
use crate::schema::routing::{BodyRequirement, BodyRouteClaim, ClaimDecoder, CollectedBody, MetadataProtocolRouter, OperationHandlerBinding, OperationTarget, RouteClaim, RouterBuildContext, RouterBuildError, RoutingOptions, SharedProtocolRouter};

pub(super) struct BoundHandler<B> {
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
pub(super) struct ProtocolRoute {
    pub(super) router: SharedProtocolRouter,
    pub(super) protocol: SharedServerProtocol,
}

/// Routing state shared by every clone of the service.
///
/// hyper-util's `TowerToHyperService` clones the service for every request, so everything here is
/// behind an `Arc`: a clone is a few reference counts, and dispatch clones only the selected route.
pub(super) struct Dispatch<B> {
    /// The served protocols in priority order. A single protocol routes with
    /// [`MetadataProtocolRouter::route`]; several claim with [`MetadataProtocolRouter::claim`].
    pub(super) protocols: Arc<[ProtocolRoute]>,
    pub(super) bindings: Arc<[BoundHandler<B>]>,
    pub(super) streaming_recognizers: Arc<[usize]>,
    pub(super) body_routers: Arc<[usize]>,
    /// The provisional allowance the service collects under for body-first routing.
    pub(super) routing_body: RequestBodyCollectionConfig,
}
impl<B> Clone for Dispatch<B> {
    fn clone(&self) -> Self {
        Self {
            protocols: self.protocols.clone(),
            bindings: self.bindings.clone(),
            streaming_recognizers: self.streaming_recognizers.clone(),
            body_routers: self.body_routers.clone(),
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
    pub(super) inner: Dispatch<B>,
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
/// No protocol owns the request, so no protocol frames the response. Coral servers answer such a
/// request with `404` and the XML body `<UnknownOperationException/>` regardless of the protocols
/// the service serves — the body is hardcoded, not produced by restXml — so services migrating
/// from Coral keep the response their clients already handle. Coral sends no `Content-Type` on
/// this response, and neither do we.
fn unclaimed() -> Response<BoxBody> {
    Response::builder()
        .status(http::StatusCode::NOT_FOUND)
        .body(crate::body::to_boxed("<UnknownOperationException/>\n"))
        .expect("a status and static body response is valid")
}

/// Reads the transport body for routing, incrementally and resumably, under the service's
/// provisional allowance.
///
/// The raw buffer is append-only and service-owned: every wire byte read while satisfying any
/// [`BodyRequirement`] accumulates here, so the transport is never read twice and
/// [`Self::into_replay_body`] can always reconstruct exactly what left the wire — for the next
/// claimant after a declined claim, or for the dispatched handler. Decoded views are derived

/// One [`BodyRequirement`] being satisfied for one protocol: the decoder's progress over the
struct Pursuit {
    /// The protocol the requirement belongs to.
    protocol: usize,
    /// `false`: an open claim, finished by [`BodyProtocolRouter::claim_with_body`].
    /// `true`: a settled claim, finished by [`BodyProtocolRouter::route_with_body`].
    routing: bool,
    decoder: Option<Box<dyn ClaimDecoder>>,
    want: BodyWant,
    /// The decoder's output so far. Unused for plain requirements.
    decoded: Vec<u8>,
    /// Raw bytes fed to the decoder so far, as an absolute offset into the raw buffer —
    /// stable across chunk consolidation.
    fed: usize,
}

impl Pursuit {
    fn new(protocol: usize, routing: bool, requirement: BodyRequirement) -> Self {
        Self {
            protocol,
            routing,
            decoder: requirement.decoder,
            want: if routing { BodyWant::Complete } else { requirement.want },
            decoded: Vec::new(),
            fed: 0,
        }
    }

    /// Feeds raw bytes the decoder has not seen yet. A no-op for plain requirements.
    fn catch_up<B>(&mut self, collector: &BodyCollector<B>) -> Result<(), BoxError> {
        let Some(decoder) = self.decoder.as_mut() else {
            return Ok(());
        };
        let mut offset = 0;
        for chunk in &collector.chunks {
            let end = offset + chunk.len();
            if end > self.fed {
                decoder.decode(&chunk[self.fed - offset..], &mut self.decoded)?;
                self.fed = end;
            }
            offset = end;
        }
        Ok(())
    }

    /// The requirement's view once it is satisfiable, `None` while more of the body is needed.
    /// At end-of-body every requirement is satisfiable (possibly short).
    fn satisfied<B>(&mut self, collector: &mut BodyCollector<B>) -> Option<CollectedBody> {
        match self.decoder {
            None => {
                let whole = collector.eof;
                match self.want {
                    BodyWant::Prefix(n) if collector.len >= n.get() => Some(CollectedBody {
                        complete: whole && collector.len <= n.get(),
                        bytes: collector.contiguous(n.get()),
                    }),
                    BodyWant::Prefix(_) | BodyWant::Complete if whole => Some(CollectedBody {
                        bytes: collector.contiguous(collector.len),
                        complete: true,
                    }),
                    BodyWant::Prefix(_) | BodyWant::Complete => None,
                }
            }
            Some(_) => {
                let whole = collector.eof && self.fed == collector.len;
                let bytes = match self.want {
                    BodyWant::Prefix(n) if self.decoded.len() >= n.get() || whole => {
                        let complete = whole && self.decoded.len() <= n.get();
                        let mut decoded = std::mem::take(&mut self.decoded);
                        decoded.truncate(n.get());
                        return Some(CollectedBody {
                            bytes: Bytes::from(decoded),
                            complete,
                        });
                    }
                    BodyWant::Complete if whole => std::mem::take(&mut self.decoded),
                    BodyWant::Prefix(_) | BodyWant::Complete => return None,
                };
                Some(CollectedBody {
                    bytes: Bytes::from(bytes),
                    complete: true,
                })
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
        selected: OperationTarget,
        protocol: usize,
        mut request: Request<crate::body::RequestBody<B>>,
    ) -> crate::routing::route::SyncRouteFuture<crate::body::RequestBody<B>> {
        let binding = &self.bindings[selected.index()];
        debug_assert!(
            std::ptr::eq(binding.operation, selected.operation()),
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
        let state = match &self.protocols[..] {
            [ProtocolRoute {
                router: SharedProtocolRouter::Metadata(router),
                ..
            }] => self.route(router, request),
            _ => {
                let (parts, body) = request.into_parts();
                self.walk(parts, BodySource::Untouched(body), ClaimWalk::default())
            }
        };
        MultiProtocolRoutingFuture { inner: state }
    }

    /// Routes with the service's only (metadata) protocol, exactly as a single-protocol service
    /// always has. A single body-routed protocol runs the claim walk instead, with the final
    /// fall-through answered as its own terminal rejection.
    fn route(&self, router: &Arc<dyn MetadataProtocolRouter>, request: Request<crate::body::RequestBody<B>>) -> State<B> {
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
            Err(err) => self.reject(0, err),
        }
    }

    /// Walks canonical order, deferring body routers for recognized streaming inputs.
    /// After metadata fall-through, deferred routers run once in their original order.
    fn walk(&self, parts: http::request::Parts, source: BodySource<B>, mut cursor: ClaimWalk) -> State<B> {
        let probe = Request::from_parts(parts, ());
        loop {
            let index = if cursor.fallback {
                let Some(index) = self.body_routers.get(cursor.next).copied() else {
                    break;
                };
                cursor.next += 1;
                index
            } else if cursor.next < self.protocols.len() {
                let index = cursor.next;
                cursor.next += 1;
                index
            } else if cursor.streaming == Some(true) {
                cursor.fallback = true;
                cursor.next = 0;
                continue;
            } else {
                break;
            };
            match &self.protocols[index].router {
                SharedProtocolRouter::Metadata(router) => match router.claim(&probe) {
                    RouteClaim::ClaimedWithRoute(selected) => {
                        let (parts, ()) = probe.into_parts();
                        return self.dispatch_replayed(selected, index, parts, source);
                    }
                    RouteClaim::Claimed => match router.route(&probe) {
                        Ok(selected) => {
                            let (parts, ()) = probe.into_parts();
                            return self.dispatch_replayed(selected, index, parts, source);
                        }
                        Err(err) => return self.reject(index, err),
                    },
                    RouteClaim::NoClaim => {}
                },
                SharedProtocolRouter::Body(router) => {
                    if !cursor.fallback {
                        let streaming = *cursor.streaming.get_or_insert_with(|| {
                            self.streaming_recognizers.iter().any(|index| {
                                let SharedProtocolRouter::Metadata(router) = &self.protocols[*index].router else {
                                    unreachable!("recognizers are metadata routers");
                                };
                                router.recognizes_streaming_input(&probe)
                            })
                        });
                        if streaming {
                            continue;
                        }
                    }
                    match router.claim(&probe) {
                        BodyRouteClaim::ClaimedWithRoute(selected) => {
                            let (parts, ()) = probe.into_parts();
                            return self.dispatch_replayed(selected, index, parts, source);
                        }
                        BodyRouteClaim::NoClaim => {}
                        BodyRouteClaim::NeedsBodyToClaim(requirement) => {
                            let (parts, ()) = probe.into_parts();
                            return self.pursue(parts, source, Pursuit::new(index, false, requirement), cursor);
                        }
                        BodyRouteClaim::Claimed(requirement) => {
                            let (parts, ()) = probe.into_parts();
                            return self.pursue(parts, source, Pursuit::new(index, true, requirement), cursor);
                        }
                    }
                }
            }
        }
        // A single body-routed protocol owns every request, so its fall-through is its own
        // terminal rejection, exactly as a single metadata protocol's `route` is.
        if self.protocols.len() == 1 {
            return self.reject(0, RoutingError::unknown_operation());
        }
        State::Rejected {
            response: Some(unclaimed()),
        }
    }

    /// Suspends the walk while the service satisfies `pursuit`'s body requirement.
    fn pursue(
        &self,
        parts: http::request::Parts,
        source: BodySource<B>,
        pursuit: Pursuit,
        cursor: ClaimWalk,
    ) -> State<B> {
        State::Collecting {
            walk: Some(WalkPursuit {
                parts,
                collector: source.into_collector(self.routing_body),
                pursuit,
                cursor,
                dispatch: self.clone(),
            }),
        }
    }

    /// Finishes a suspended pursuit over its satisfied requirement, synchronously.
    fn resumed(
        &self,
        parts: http::request::Parts,
        collector: BodyCollector<B>,
        pursuit: &Pursuit,
        collected: CollectedBody,
        cursor: ClaimWalk,
    ) -> State<B> {
        let SharedProtocolRouter::Body(router) = &self.protocols[pursuit.protocol].router else {
            unreachable!("only body-routed protocols suspend the walk");
        };
        let request = Request::from_parts(parts, collected);
        if pursuit.routing {
            return match router.route_with_body(&request) {
                Ok(selected) => {
                    let (parts, _) = request.into_parts();
                    self.dispatch_replayed(selected, pursuit.protocol, parts, BodySource::Collector(collector))
                }
                Err(err) => self.reject(pursuit.protocol, err),
            };
        }
        match router.claim_with_body(&request) {
            BodyRouteClaim::ClaimedWithRoute(selected) => {
                let (parts, _) = request.into_parts();
                self.dispatch_replayed(selected, pursuit.protocol, parts, BodySource::Collector(collector))
            }
            BodyRouteClaim::NoClaim => {
                let (parts, _) = request.into_parts();
                self.walk(parts, BodySource::Collector(collector), cursor)
            }
            BodyRouteClaim::NeedsBodyToClaim(requirement) => {
                let (parts, _) = request.into_parts();
                self.pursue(
                    parts,
                    BodySource::Collector(collector),
                    Pursuit::new(pursuit.protocol, false, requirement),
                    cursor,
                )
            }
            BodyRouteClaim::Claimed(requirement) => {
                let (parts, _) = request.into_parts();
                self.pursue(
                    parts,
                    BodySource::Collector(collector),
                    Pursuit::new(pursuit.protocol, true, requirement),
                    cursor,
                )
            }
        }
    }

    /// Frames a router's rejection with the rejecting protocol's own modeled-error serialization.
    fn reject(&self, protocol: usize, err: RoutingError) -> State<B> {
        State::Rejected {
            response: Some(self.protocols[protocol].protocol.serialize_routing_error(&err)),
        }
    }

    /// Hands a routed request to its handler, replaying anything routing read off the wire.
    fn dispatch_replayed(
        &self,
        selected: OperationTarget,
        protocol: usize,
        parts: http::request::Parts,
        source: BodySource<B>,
    ) -> State<B> {
        let body = source.into_replay_body();
        State::Handling {
            future: self.handle(selected, protocol, Request::from_parts(parts, body)),
        }
    }
}

/// Where the request body stands when the walk needs it: still the untouched pipeline body, or
/// already (partially) read into the service's collector.
enum BodySource<B> {
    Untouched(crate::body::RequestBody<B>),
    Collector(BodyCollector<B>),
}

impl<B> BodySource<B> {
    fn into_collector(self, config: RequestBodyCollectionConfig) -> BodyCollector<B> {
        match self {
            Self::Untouched(body) => BodyCollector::new(body, config),
            Self::Collector(collector) => collector,
        }
    }

    fn into_replay_body(self) -> crate::body::RequestBody<B> {
        match self {
            Self::Untouched(body) => body,
            Self::Collector(collector) => collector.into_replay_body(),
        }
    }
}

/// Request-local cursor retained across collection. Recognition is computed at most once.
#[derive(Default)]
struct ClaimWalk {
    next: usize,
    streaming: Option<bool>,
    fallback: bool,
}

/// A suspended walk: the request head, the service's collector, and the pursuit being
/// satisfied. Owns everything — nothing is borrowed across the suspension — and is `Unpin`,
/// so pursuits move freely between walk states.
struct WalkPursuit<B> {
    parts: http::request::Parts,
    collector: BodyCollector<B>,
    pursuit: Pursuit,
    cursor: ClaimWalk,
    dispatch: Dispatch<B>,
}

enum PursuitPoll {
    /// The requirement is met; resume the walk over this view.
    Satisfied(CollectedBody),
    /// The pursuit's decoder rejected the bytes.
    DecodeFailed(BoxError),
    /// The transport failed, timed out, or overran the provisional allowance.
    CollectionFailed(crate::schema::RequestBodyCollectionError<crate::Error>),
}

impl<B> WalkPursuit<B>
where
    B: http_body::Body<Data = Bytes> + Unpin,
    B::Error: Into<BoxError>,
{
    fn poll_pursuit(&mut self, cx: &mut Context<'_>) -> Poll<PursuitPoll> {
        loop {
            if let Err(err) = self.pursuit.catch_up(&self.collector) {
                return Poll::Ready(PursuitPoll::DecodeFailed(err));
            }
            if let Some(collected) = self.pursuit.satisfied(&mut self.collector) {
                return Poll::Ready(PursuitPoll::Satisfied(collected));
            }
            match self.collector.poll_read(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(err)) => return Poll::Ready(PursuitPoll::CollectionFailed(err)),
                Poll::Ready(Ok(())) => {}
            }
        }
    }
}

pin_project_lite::pin_project! {
    #[project = StateProj]
    enum State<B> {
        // A walk suspended on a body requirement. `Option` so completion can take ownership;
        // the pursuit is `Unpin`, so no structural pinning is needed.
        Collecting {
            walk: Option<WalkPursuit<B>>,
        },
        Handling {
            #[pin]
            future: crate::routing::route::SyncRouteFuture<crate::body::RequestBody<B>>,
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
                StateProj::Collecting { walk } => {
                    let outcome = match walk.as_mut().expect("pursuit resolves once").poll_pursuit(cx) {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(outcome) => outcome,
                    };
                    let WalkPursuit {
                        parts,
                        collector,
                        pursuit,
                        cursor,
                        dispatch,
                    } = walk.take().expect("pursuit resolves once");
                    match outcome {
                        PursuitPoll::Satisfied(collected) => {
                            this.inner
                                .set(dispatch.resumed(parts, collector, &pursuit, collected, cursor));
                        }
                        // The decoder rejecting the bytes means different things by phase: an
                        // open claim was simply not this protocol's — the walk continues over
                        // the replayable buffer — while a settled claim owns the request, so
                        // a body its own codec cannot decode is its malformed request.
                        PursuitPoll::DecodeFailed(err) => {
                            if pursuit.routing {
                                this.inner.set(dispatch.reject(
                                    pursuit.protocol,
                                    RoutingError::malformed(crate::Error::new(err)),
                                ));
                            } else {
                                this.inner
                                    .set(dispatch.walk(parts, BodySource::Collector(collector), cursor));
                            }
                        }
                        // A transport-level failure is terminal whatever the phase; the
                        // protocol the requirement belonged to frames it, exactly as when it
                        // collected for itself.
                        PursuitPoll::CollectionFailed(error) => {
                            let protocol = &dispatch.protocols[pursuit.protocol].protocol;
                            return Poll::Ready(Ok(crate::schema::body_collection_rejection(&**protocol, error)));
                        }
                    }
                }
                StateProj::Handling { future } => return future.poll(cx),
                StateProj::Rejected { response } => {
                    return Poll::Ready(Ok(response.take().expect("polled after completion")))
                }
            }
        }
    }
}

/// Resolves the registered protocols the service declares, in claim order.
///
/// The order comes from [`ProtocolOrder`] constraints alone, resolved over the **global** set of
/// registered protocols: a constraint against a protocol the service does not serve still orders
/// the ones it does, transitively. Registry and declaration order carry no meaning. Errors:
/// a declared protocol without a registration, a protocol registered twice, a constraint naming
/// an unregistered protocol, a constraint cycle, or two served protocols left unordered.
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

    // Validate every declaration before invoking any protocol factory or building its router.
    let missing: Vec<String> = service
        .protocols()
        .iter()
        .filter(|protocol| {
            !registrations
                .iter()
                .any(|registration| registration.protocol_id() == protocol.as_str())
        })
        .map(|protocol| protocol.to_string())
        .collect();
    if !missing.is_empty() {
        return Err(RouterBuildError::MissingProtocols { protocols: missing });
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
            .map(|(index, binding)| OperationTarget::new(index, binding.operation))
            .collect();
        // A body-routed protocol may buffer the body to select, so it never sees a streaming
        // operation. Streaming-input recognition defers it until metadata routers have passed.
        let non_streaming: Vec<_> = targets
            .iter()
            .filter(|target| !target.has_streaming_input() && !target.has_streaming_output())
            .copied()
            .collect();
        // `resolved` is already in claim order; build each protocol's router in place. The
        // protocol's kind picks its target set inside `build_router`: a body-routed protocol
        // gets only the non-streaming operations.
        let mut protocols = Vec::with_capacity(resolved.len());
        for protocol in resolved {
            let router = protocol.build_router(
                RouterBuildContext {
                    service,
                    targets: &targets,
                    config: &options.request_body,
                    protocol_settings: options.protocol_settings.get(protocol.protocol_id().as_str()),
                },
                &non_streaming,
            )?;
            protocols.push(ProtocolRoute { router, protocol });
        }
        // Whether recognition is needed comes from the service schema. Each metadata
        // router owns recognition of the streaming operations its protocol supports.
        let has_streaming_inputs = targets.iter().any(|target| target.has_streaming_input());
        let streaming_recognizers = protocols
            .iter()
            .enumerate()
            .filter_map(|(index, route)| match &route.router {
                SharedProtocolRouter::Metadata(_) if has_streaming_inputs => Some(index),
                _ => None,
            })
            .collect::<Vec<_>>();
        let body_routers = protocols
            .iter()
            .enumerate()
            .filter_map(|(index, route)| matches!(route.router, SharedProtocolRouter::Body(_)).then_some(index))
            .collect::<Vec<_>>();
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
                streaming_recognizers: streaming_recognizers.into(),
                body_routers: body_routers.into(),
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
