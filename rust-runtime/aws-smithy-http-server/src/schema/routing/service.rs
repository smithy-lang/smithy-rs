/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The multi-protocol routing service: dispatch state, the claim walk, and its futures.

use super::MultiProtocolRoutingServiceBuilder;
use crate::routing::SyncRoute;
use crate::schema::routing::RoutingError;
use crate::schema::{OperationSchema, ServiceSchema};
use crate::{
    body::BoxBody,
    error::BoxError,
    schema::{RequestBodyCollectionConfig, SelectedOperation, SharedServerProtocol},
};
use bytes::Bytes;
use http::{Request, Response};

use crate::schema::routing::{
    BodyRouteClaim, CollectedBody, MetadataProtocolRouter, OperationTarget, RouteClaim, SharedProtocolRouter,
};
use std::{
    convert::Infallible,
    fmt,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tower::Service;

pub(super) struct BoundHandler {
    pub(super) operation: &'static OperationSchema<'static>,
    pub(super) collection_config: RequestBodyCollectionConfig,
    pub(super) handler_route: SyncRoute<crate::body::RequestBody>,
}
impl Clone for BoundHandler {
    fn clone(&self) -> Self {
        Self {
            operation: self.operation,
            collection_config: self.collection_config,
            handler_route: self.handler_route.clone(),
        }
    }
}
impl fmt::Debug for BoundHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoundHandler")
            .field("operation", &self.operation.shape_id())
            .finish()
    }
}

/// A served protocol and the router it built.
#[derive(Debug)]
pub(super) struct ProtocolAndRouter {
    pub(super) router: SharedProtocolRouter,
    pub(super) protocol: SharedServerProtocol,
}

/// A service routing normalized requests to a shared handler collection through the protocols the
/// service declares.
///
/// Hyper incoming bodies flow to handlers without boxing. Other compatible request bodies
/// are accepted and erased into a boxed state on entry; normalized request bodies retain
/// their existing representation.
///
/// Clones share all routing state through one `Arc`. Routing clones only the
/// selected handler, so cloning the service for each request stays cheap.
pub struct MultiProtocolRoutingService {
    pub(super) state: Arc<RoutingState>,
}

/// Routers, handlers, and configuration shared by service clones and in-flight requests.
pub(super) struct RoutingState {
    /// The served protocols in priority order. Every configuration checks claims;
    /// a single metadata router can do so without an async routing loop.
    pub(super) protocols: Box<[ProtocolAndRouter]>,
    pub(super) handlers: Box<[BoundHandler]>,
    /// Indices into `protocols` of metadata routers checked for streaming inputs before body
    /// collection. `None` when the service has no streaming-input operations; otherwise includes
    /// all metadata routers. Recognized streaming inputs skip claims that require body I/O.
    pub(super) metadata_routers: Option<Box<[usize]>>,
    /// The provisional allowance the service collects under for body-first routing.
    pub(super) body_collection_config: RequestBodyCollectionConfig,
}
impl Clone for MultiProtocolRoutingService {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}
impl fmt::Debug for MultiProtocolRoutingService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MultiProtocolRoutingService")
            .field("protocols", &self.state.protocols)
            .field("handlers", &self.state.handlers)
            .finish()
    }
}

/// The response when no protocol claims the request or offers a deferred rejection.
///
/// No protocol owns the request, so no protocol frames the response. Return `404` with
/// the fixed XML body `<UnknownOperationException/>\n` and no `Content-Type` header,
/// regardless of the protocols the service serves.
fn unclaimed() -> Response<BoxBody> {
    Response::builder()
        .status(http::StatusCode::NOT_FOUND)
        .body(crate::body::to_boxed("<UnknownOperationException/>\n"))
        .expect("a status and static body response is valid")
}

impl MultiProtocolRoutingService {
    /// Hands the routed request to its handler, recording the selection for downstream consumers.
    fn handle(
        &self,
        selected: OperationTarget,
        protocol: usize,
        mut request: Request<crate::body::RequestBody>,
    ) -> crate::routing::route::SyncRouteFuture<crate::body::RequestBody> {
        let binding = &self.state.handlers[selected.index()];
        debug_assert!(
            std::ptr::eq(binding.operation, selected.operation()),
            "router index belongs to a different operation"
        );
        SelectedOperation::new(
            self.state.protocols[protocol].protocol.clone(),
            binding.operation,
            binding.collection_config,
        )
        .insert_into(request.extensions_mut());
        binding.handler_route.clone().call_owned(request)
    }

    fn route_request(&self, request: Request<crate::body::RequestBody>) -> MultiProtocolRoutingFuture {
        match &self.state.protocols[..] {
            // A single metadata router can check its claim synchronously.
            [ProtocolAndRouter {
                router: SharedProtocolRouter::Metadata(router),
                ..
            }] => MultiProtocolRoutingFuture {
                inner: self.route(router.as_ref(), request),
                routing: None,
            },
            _ => {
                let (parts, body) = request.into_parts();
                MultiProtocolRoutingFuture {
                    inner: State::Routing,
                    routing: Some(ProtocolRouting {
                        service: self.clone(),
                        probe: Request::from_parts(parts, ()),
                        body: Some(body),
                        collected: None,
                        index: 0,
                        streaming: None,
                        deferred: None,
                        deferred_claim: None,
                    }),
                }
            }
        }
    }

    /// Checks the service's only metadata protocol without allocating a routing future.
    fn route(&self, router: &dyn MetadataProtocolRouter, request: Request<crate::body::RequestBody>) -> State {
        // Probe with the head only: the parts move over and back, nothing is cloned,
        // and the router stays free of the transport body type.
        let (parts, body) = request.into_parts();
        let probe = Request::from_parts(parts, ());
        let selected = match router.claim(&probe) {
            RouteClaim::ClaimedWithRoute(selected) | RouteClaim::DeferredClaim(selected) => Ok(selected),
            RouteClaim::Claimed => router.route(&probe),
            RouteClaim::DeferredRejection(error) => Err(error),
            RouteClaim::NoClaim => {
                return State::Rejected {
                    response: Some(unclaimed()),
                }
            }
        };
        match selected {
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

    /// Frames a routing error with the rejecting protocol's serialization.
    fn reject(&self, protocol: usize, error: RoutingError) -> Response<BoxBody> {
        self.state.protocols[protocol].protocol.serialize_routing_error(&error)
    }
}

/// The claim walk remains owned until the response completes, including handler readiness.
struct ProtocolRouting {
    service: MultiProtocolRoutingService,
    probe: Request<()>,
    body: Option<crate::body::RequestBody>,
    collected: Option<Bytes>,
    index: usize,
    streaming: Option<bool>,
    deferred: Option<(usize, RoutingError)>,
    deferred_claim: Option<(usize, OperationTarget)>,
}

impl ProtocolRouting {
    /// Continues synchronously until body I/O or handler readiness must be polled.
    fn advance(&mut self) -> State {
        while self.index < self.service.state.protocols.len() {
            let index = self.index;
            let protocol = &self.service.state.protocols[index];
            let selected = match &protocol.router {
                SharedProtocolRouter::Metadata(router) => match router.claim(&self.probe) {
                    RouteClaim::ClaimedWithRoute(selected) => Some(Ok(selected)),
                    RouteClaim::DeferredClaim(selected) => {
                        self.deferred_claim.get_or_insert((index, selected));
                        None
                    }
                    RouteClaim::Claimed => Some(router.route(&self.probe)),
                    RouteClaim::DeferredRejection(error) => {
                        self.deferred.get_or_insert((index, error));
                        None
                    }
                    RouteClaim::NoClaim => None,
                },
                SharedProtocolRouter::Body(router) => {
                    let claim = router.claim(&self.probe);
                    match claim {
                        BodyRouteClaim::ClaimedWithRoute(selected) => Some(Ok(selected)),
                        BodyRouteClaim::DeferredClaim(selected) => {
                            self.deferred_claim.get_or_insert((index, selected));
                            None
                        }
                        BodyRouteClaim::NoClaim => None,
                        BodyRouteClaim::DeferredRejection(error) => {
                            self.deferred.get_or_insert((index, error));
                            None
                        }
                        BodyRouteClaim::NeedsBodyToClaim | BodyRouteClaim::Claimed => {
                            let needs_body_to_claim = matches!(claim, BodyRouteClaim::NeedsBodyToClaim);
                            if needs_body_to_claim
                                && *self.streaming.get_or_insert_with(|| {
                                    self.service.state.metadata_routers.as_ref().is_some_and(|indices| {
                                        indices.iter().any(|index| {
                                            let SharedProtocolRouter::Metadata(router) =
                                                &self.service.state.protocols[*index].router
                                            else {
                                                unreachable!("recognizers are metadata routers");
                                            };
                                            router.recognizes_streaming_input(&self.probe)
                                        })
                                    })
                                })
                            {
                                tracing::trace!(protocol = %protocol.protocol.protocol_id(), "skipping protocol that needs body bytes to claim a request recognized as streaming");
                                None
                            } else if self.collected.is_none() {
                                return State::Collecting {
                                    future: crate::schema::protocol::collect_request_body_with_trailers(
                                        self.body.take().expect("body is collected once"),
                                        &self.service.state.body_collection_config,
                                    ),
                                    needs_body_to_claim,
                                };
                            } else {
                                self.select_body(needs_body_to_claim)
                            }
                        }
                    }
                }
            };
            if let Some(selected) = selected {
                return self.dispatch(selected);
            }
            self.index += 1;
        }
        if let Some((index, selected)) = self.deferred_claim.take() {
            self.index = index;
            return self.dispatch(Ok(selected));
        }
        State::Rejected {
            response: Some(match self.deferred.take() {
                Some((index, error)) => self.service.reject(index, error),
                None => unclaimed(),
            }),
        }
    }

    /// Resumes the same body claim after collection, without probing the head again.
    fn select_body(&mut self, needs_body_to_claim: bool) -> Option<Result<OperationTarget, RoutingError>> {
        let SharedProtocolRouter::Body(router) = &self.service.state.protocols[self.index].router else {
            unreachable!("only body routers collect for routing");
        };
        let probe = std::mem::replace(&mut self.probe, Request::new(()));
        let request = probe.map(|()| CollectedBody {
            bytes: self.collected.as_ref().expect("body was collected").clone(),
        });
        let claim = if needs_body_to_claim {
            router.claim_with_body(&request)
        } else {
            RouteClaim::Claimed
        };
        let selected = match claim {
            RouteClaim::ClaimedWithRoute(selected) => Some(Ok(selected)),
            RouteClaim::DeferredClaim(selected) => {
                self.deferred_claim.get_or_insert((self.index, selected));
                None
            }
            RouteClaim::Claimed => Some(router.route_with_body(&request)),
            RouteClaim::NoClaim => None,
            RouteClaim::DeferredRejection(error) => {
                self.deferred.get_or_insert((self.index, error));
                None
            }
        };
        self.probe = request.map(|_| ());
        selected
    }

    fn dispatch(&mut self, selected: Result<OperationTarget, RoutingError>) -> State {
        match selected {
            Ok(selected) => {
                let probe = std::mem::replace(&mut self.probe, Request::new(()));
                State::Handling {
                    future: self.service.handle(
                        selected,
                        self.index,
                        probe.map(|()| self.body.take().expect("body remains until dispatch")),
                    ),
                }
            }
            Err(error) => State::Rejected {
                response: Some(self.service.reject(self.index, error)),
            },
        }
    }
}

pin_project_lite::pin_project! {
    #[project = StateProj]
    enum State {
        Routing,
        Collecting {
            #[pin]
            future: crate::schema::protocol::RequestBodyCollectionFuture<crate::body::RequestBody>,
            needs_body_to_claim: bool,
        },
        Handling {
            #[pin]
            future: crate::routing::route::SyncRouteFuture<crate::body::RequestBody>,
        },
        Rejected {
            response: Option<Response<BoxBody>>,
        },
        Done,
    }
}

pin_project_lite::pin_project! {
    /// Response future for schema routing.
    pub struct MultiProtocolRoutingFuture {
        #[pin]
        inner: State,
        routing: Option<ProtocolRouting>,
    }
}
impl Future for MultiProtocolRoutingFuture {
    type Output = Result<Response<BoxBody>, Infallible>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        loop {
            let next = match this.inner.as_mut().project() {
                StateProj::Routing => this.routing.as_mut().expect("claim walk exists").advance(),
                StateProj::Collecting {
                    future,
                    needs_body_to_claim,
                } => {
                    let routing = this.routing.as_mut().expect("claim walk exists");
                    match std::task::ready!(future.poll(cx)) {
                        Ok((bytes, trailers)) => {
                            routing.body = Some(crate::body::RequestBody::buffered(bytes.clone(), trailers));
                            routing.collected = Some(bytes);
                            match routing.select_body(*needs_body_to_claim) {
                                Some(selected) => routing.dispatch(selected),
                                None => {
                                    routing.index += 1;
                                    State::Routing
                                }
                            }
                        }
                        Err(error) => State::Rejected {
                            response: Some(crate::schema::body_collection_rejection(
                                &*routing.service.state.protocols[routing.index].protocol,
                                error,
                            )),
                        },
                    }
                }
                StateProj::Handling { future } => {
                    let response = std::task::ready!(future.poll(cx));
                    this.inner.set(State::Done);
                    *this.routing = None;
                    return Poll::Ready(response);
                }
                StateProj::Rejected { response } => {
                    let response = response.take().expect("polled after completion");
                    this.inner.set(State::Done);
                    *this.routing = None;
                    return Poll::Ready(Ok(response));
                }
                StateProj::Done => panic!("polled after completion"),
            };
            this.inner.set(next);
        }
    }
}

impl MultiProtocolRoutingService {
    /// Starts configuring routing for the supplied service schema.
    pub fn builder(service: &'static ServiceSchema<'static>) -> MultiProtocolRoutingServiceBuilder {
        MultiProtocolRoutingServiceBuilder::new(service)
    }
}
/// Any compatible body enters. A hyper incoming body and an already-normalized
/// [`RequestBody`](crate::body::RequestBody) stay unerased; any other body — tests, adapters,
/// upgrade layers — is erased into a boxed state (see [`RequestBody::new`](crate::body::RequestBody::new)).
impl<RB> Service<Request<RB>> for MultiProtocolRoutingService
where
    RB: http_body::Body<Data = Bytes> + Send + Sync + 'static,
    RB::Error: Into<BoxError>,
{
    type Response = Response<BoxBody>;
    type Error = Infallible;
    type Future = MultiProtocolRoutingFuture;
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }
    fn call(&mut self, request: Request<RB>) -> Self::Future {
        self.route_request(request.map(crate::body::RequestBody::new))
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::{binding, FIRST, REST_JSON, SECOND};
    use super::*;
    use crate::body::Body;
    use http::{HeaderValue, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn normalized_and_boxed_bodies_preserve_shared_routing() {
        use crate::body::RequestBody;
        use aws_smithy_schema::shape_id;
        use http_body_util::BodyExt;

        static MULTI: ServiceSchema<'static> = ServiceSchema::new(
            shape_id!("test", "Service"),
            None,
            &[
                shape_id!("aws.protocols", "restJson1"),
                shape_id!("aws.protocols", "restXml"),
            ],
            &[&FIRST, &SECOND],
        );

        fn route() -> SyncRoute<RequestBody> {
            SyncRoute::new(tower::service_fn(|request: Request<RequestBody>| async {
                assert!(SelectedOperation::get_from_request(&request).is_some());
                let bytes = request.into_body().collect().await.unwrap().to_bytes();
                Ok::<_, Infallible>(Response::new(crate::body::to_boxed(bytes)))
            }))
        }

        // Normalized bodies and shared handler services retain their thread-safety guarantees.
        crate::test_helpers::assert_send::<MultiProtocolRoutingService>();
        crate::test_helpers::assert_sync::<MultiProtocolRoutingService>();
        crate::test_helpers::assert_sync::<SyncRoute<RequestBody>>();

        for schema in [&REST_JSON, &MULTI] {
            let service = MultiProtocolRoutingServiceBuilder::new(schema)
                .operation_handler_bindings([(&FIRST, route()), (&SECOND, route())])
                .build()
                .unwrap();
            let cloned = service.clone();
            assert!(Arc::ptr_eq(&service.state, &cloned.state));

            let request = Request::builder()
                .method("POST")
                .uri("/first")
                .header("content-type", "application/json")
                .body(http_body_util::Full::new(Bytes::from_static(b"payload")))
                .unwrap();
            let normalized = Request::builder()
                .method("POST")
                .uri("/second")
                .header("content-type", "application/json")
                .body(RequestBody::from_bytes(Bytes::from_static(b"normalized")))
                .unwrap();

            // Exercise public ingress and the buffered normalized body concurrently,
            // through both the single-protocol shortcut and the async claim walk.
            let ingress = tokio::spawn(cloned.oneshot(request));
            let routing = tokio::spawn(service.route_request(normalized));
            let response = ingress.await.unwrap().unwrap();
            assert_eq!(response.into_body().collect().await.unwrap().to_bytes(), "payload");
            let response = routing.await.unwrap().unwrap();
            assert_eq!(response.into_body().collect().await.unwrap().to_bytes(), "normalized");
        }
    }

    #[tokio::test]
    async fn builder_defers_layers_until_build_and_preserves_stack_order() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let applications = Arc::new(AtomicUsize::new(0));
        let counter = applications.clone();

        let layer = tower::layer::layer_fn(move |inner: SyncRoute<Body>| {
            counter.fetch_add(1, Ordering::Release);

            tower::service_fn(move |mut request: Request<Body>| {
                assert!(SelectedOperation::get_from_request(&request).is_some());
                assert_eq!(request.headers()["x-layer-order"], "outer");
                request
                    .headers_mut()
                    .insert("x-layer-order", HeaderValue::from_static("inner"));
                inner.clone().oneshot(request)
            })
        });

        let outer = tower::util::MapRequestLayer::new(|mut request: Request<Body>| {
            assert!(!request.headers().contains_key("x-layer-order"));
            request
                .headers_mut()
                .insert("x-layer-order", HeaderValue::from_static("outer"));
            request
        });

        let stack = tower::layer::util::Stack::new(layer, outer);
        let builder = MultiProtocolRoutingService::builder(&REST_JSON)
            .operation_handler_bindings([binding(&FIRST), binding(&SECOND)])
            .layer(stack);
        assert_eq!(applications.load(Ordering::Acquire), 0);

        let app = builder.build().unwrap();
        assert_eq!(applications.load(Ordering::Acquire), 2);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/first")
                    .header("content-type", "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
