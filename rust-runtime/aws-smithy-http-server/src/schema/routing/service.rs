/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The multi-protocol routing service: dispatch state, the claim walk, and its futures.

use crate::routing::SyncRoute;
use crate::schema::routing::RoutingError;
use crate::schema::{OperationSchema, ServiceSchema};
use crate::{
    body::BoxBody,
    error::BoxError,
    schema::{
        ProtocolBuildContext, ProtocolOrder, ProtocolRegistration, ProtocolRegistry, RequestBodyCollectionConfig,
        SelectedProtocolOperation, SharedServerProtocol,
    },
};
use bytes::Bytes;
use http::{Request, Response};

use crate::schema::routing::{
    BodyRouteClaim, CollectedBody, MetadataProtocolRouter, OperationHandlerBinding, OperationTarget, RouteClaim,
    RouterBuildContext, RouterBuildError, RoutingOptions, SharedProtocolRouter,
};
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

pub(super) struct BoundHandler<B> {
    operation: &'static OperationSchema<'static>,
    collection_config: RequestBodyCollectionConfig,
    handler_route: SyncRoute<crate::body::RequestBody<B>>,
}
impl<B> Clone for BoundHandler<B> {
    fn clone(&self) -> Self {
        Self {
            operation: self.operation,
            collection_config: self.collection_config,
            handler_route: self.handler_route.clone(),
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
pub(super) struct ProtocolAndRouter {
    pub(super) router: SharedProtocolRouter,
    pub(super) protocol: SharedServerProtocol,
}

/// A service routing normalized requests to a shared handler collection through the protocols the
/// service declares.
///
/// Generic over the transport body `B`: requests entering with the transport's own body flow to
/// handlers unerased. The default is hyper's body; any other request body — tests, upgrade
/// layers, other transports — is accepted and erased into a boxed state on entry.
///
/// Clones share all routing state through one `Arc`. Routing clones only the
/// selected handler, so cloning the service for each request stays cheap.
pub struct MultiProtocolRoutingService<B = hyper::body::Incoming> {
    pub(super) state: Arc<RoutingState<B>>,
}

/// Routers, handlers, and configuration shared by service clones and in-flight requests.
pub(super) struct RoutingState<B> {
    /// The served protocols in priority order. A single protocol routes with
    /// [`MetadataProtocolRouter::route`]; several claim with [`MetadataProtocolRouter::claim`].
    pub(super) protocols: Box<[ProtocolAndRouter]>,
    pub(super) handlers: Box<[BoundHandler<B>]>,
    /// Indices into `protocols` of metadata routers checked for streaming inputs before body
    /// collection. `None` when the service has no streaming-input operations; otherwise includes
    /// all metadata routers. Recognized streaming inputs skip claims that require body I/O.
    pub(super) metadata_routers: Option<Box<[usize]>>,
    /// The provisional allowance the service collects under for body-first routing.
    pub(super) body_collection_config: RequestBodyCollectionConfig,
}
impl<B> Clone for MultiProtocolRoutingService<B> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}
impl<B> fmt::Debug for MultiProtocolRoutingService<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MultiProtocolRoutingService")
            .field("protocols", &self.state.protocols)
            .field("handlers", &self.state.handlers)
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

impl<B> MultiProtocolRoutingService<B>
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
        let binding = &self.state.handlers[selected.index()];
        debug_assert!(
            std::ptr::eq(binding.operation, selected.operation()),
            "router index belongs to a different operation"
        );
        request.extensions_mut().insert(SelectedProtocolOperation::new(
            self.state.protocols[protocol].protocol.clone(),
            binding.operation,
            binding.collection_config,
        ));
        binding.handler_route.clone().call_owned(request)
    }

    fn route_request(&self, request: Request<crate::body::RequestBody<B>>) -> MultiProtocolRoutingFuture<B> {
        let state = match &self.state.protocols[..] {
            // Do we have only one protocol and that is a MetaData router?
            [ProtocolAndRouter {
                router: SharedProtocolRouter::Metadata(router),
                ..
            }] => self.route(router, request),
            _ => State::Routing {
                // TODO: Investigate avoiding this Arc clone when routing needs no body I/O.
                future: Box::pin(self.clone().route_protocols(request)),
            },
        };
        MultiProtocolRoutingFuture { inner: state }
    }

    /// Routes with the service's only (metadata) protocol, exactly as a single-protocol service
    /// always has. A single body-routed protocol runs the routing loop instead, with the final
    /// fall-through answered as its own terminal rejection.
    fn route(
        &self,
        router: &Arc<dyn MetadataProtocolRouter>,
        request: Request<crate::body::RequestBody<B>>,
    ) -> State<B> {
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
            Err(err) => State::Rejected {
                response: Some(self.reject(0, err)),
            },
        }
    }

    /// Tries protocols in canonical order, retaining the collected body if a router declines.
    async fn route_protocols(
        self,
        request: Request<crate::body::RequestBody<B>>,
    ) -> Result<Response<BoxBody>, Infallible> {
        let (parts, mut body) = request.into_parts();
        let mut probe = Request::from_parts(parts, ());
        let mut collected: Option<Bytes> = None;
        let mut streaming = None;

        for (index, protocol) in self.state.protocols.iter().enumerate() {
            let selected = match &protocol.router {
                SharedProtocolRouter::Metadata(router) => match router.claim(&probe) {
                    RouteClaim::ClaimedWithRoute(selected) => Ok(selected),
                    RouteClaim::Claimed => router.route(&probe),
                    RouteClaim::NoClaim => continue,
                },
                SharedProtocolRouter::Body(router) => {
                    let claim = router.claim(&probe);
                    match claim {
                        BodyRouteClaim::ClaimedWithRoute(selected) => Ok(selected),
                        BodyRouteClaim::NoClaim => continue,
                        BodyRouteClaim::NeedsBodyToClaim | BodyRouteClaim::Claimed => {
                            // A streaming operation cannot be given to a body claiming protocol.
                            if matches!(claim, BodyRouteClaim::NeedsBodyToClaim)
                                && *streaming.get_or_insert_with(|| {
                                    self.state.metadata_routers.as_ref().is_some_and(|indices| {
                                        indices.iter().any(|index| {
                                            let SharedProtocolRouter::Metadata(router) =
                                                &self.state.protocols[*index].router
                                            else {
                                                unreachable!("recognizers are metadata routers");
                                            };
                                            router.recognizes_streaming_input(&probe)
                                        })
                                    })
                                })
                            {
                                tracing::trace!(
                                    protocol = %protocol.protocol.protocol_id(),
                                    "skipping protocol that needs body bytes to claim a request recognized as streaming"
                                );
                                continue;
                            }
                            if collected.is_none() {
                                let (bytes, trailers) =
                                    match crate::schema::protocol::collect_request_body_with_trailers(
                                        body,
                                        &self.state.body_collection_config,
                                    )
                                    .await
                                    {
                                        Ok(collected) => collected,
                                        Err(error) => {
                                            return Ok(crate::schema::body_collection_rejection(
                                                &*self.state.protocols[index].protocol,
                                                error,
                                            ))
                                        }
                                    };
                                body = crate::body::RequestBody::buffered(bytes.clone(), trailers);
                                collected = Some(bytes);
                            }
                            let request = probe.map(|()| CollectedBody {
                                bytes: collected.as_ref().expect("body was collected").clone(),
                            });
                            let claim = match claim {
                                BodyRouteClaim::NeedsBodyToClaim => router.claim_with_body(&request),
                                BodyRouteClaim::Claimed => RouteClaim::Claimed,
                                _ => unreachable!("handled head-only claims above"),
                            };
                            let selected = match claim {
                                RouteClaim::ClaimedWithRoute(selected) => Some(Ok(selected)),
                                RouteClaim::Claimed => Some(router.route_with_body(&request)),
                                RouteClaim::NoClaim => None,
                            };
                            probe = request.map(|_| ());
                            let Some(selected) = selected else { continue };
                            selected
                        }
                    }
                }
            };
            return match selected {
                Ok(selected) => self.handle(selected, index, probe.map(|()| body)).await,
                Err(error) => Ok(self.reject(index, error)),
            };
        }
        // A single body protocol owns even requests it does not recognize.
        Ok(if self.state.protocols.len() == 1 {
            self.reject(0, RoutingError::unknown_operation())
        } else {
            unclaimed()
        })
    }

    /// Frames a routing error with the rejecting protocol's serialization.
    fn reject(&self, protocol: usize, error: RoutingError) -> Response<BoxBody> {
        self.state.protocols[protocol].protocol.serialize_routing_error(&error)
    }
}

pin_project_lite::pin_project! {
    #[project = StateProj]
    enum State<B> {
        Routing {
            future: Pin<Box<dyn Future<Output = Result<Response<BoxBody>, Infallible>> + Send>>,
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
        match self.project().inner.project() {
            StateProj::Routing { future } => future.as_mut().poll(cx),
            StateProj::Handling { future } => future.poll(cx),
            StateProj::Rejected { response } => Poll::Ready(Ok(response.take().expect("polled after completion"))),
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
    let position = |id: &str| {
        registrations
            .iter()
            .position(|registration| registration.protocol_id() == id)
    };
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
            protocols.push(ProtocolAndRouter { router, protocol });
        }
        // Whether recognition is needed comes from the service schema. Each metadata
        // router owns recognition of the streaming operations its protocol supports.
        let has_streaming_inputs = targets.iter().any(|target| target.has_streaming_input());
        let metadata_routers = has_streaming_inputs.then(|| {
            protocols
                .iter()
                .enumerate()
                .filter_map(|(index, route)| matches!(route.router, SharedProtocolRouter::Metadata(_)).then_some(index))
                .collect::<Box<[_]>>()
        });
        let bindings = bindings
            .into_iter()
            .map(|binding| BoundHandler {
                operation: binding.operation,
                collection_config: options.request_body.for_operation(binding.operation.shape_id()),
                handler_route: binding.route,
            })
            .collect();
        Ok(Self {
            state: Arc::new(RoutingState {
                protocols: protocols.into(),
                metadata_routers,
                handlers: bindings,
                body_collection_config: options.request_body.for_routing(),
            }),
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
        let handlers = self
            .state
            .handlers
            .iter()
            .cloned()
            .map(|binding| BoundHandler {
                operation: binding.operation,
                collection_config: binding.collection_config,
                handler_route: SyncRoute::new(layer.layer(binding.handler_route)),
            })
            .collect();
        self.state = Arc::new(RoutingState {
            protocols: self.state.protocols.clone(),
            handlers,
            metadata_routers: self.state.metadata_routers.clone(),
            body_collection_config: self.state.body_collection_config,
        });
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
        self.route_request(request.map(crate::body::RequestBody::new))
    }
}
