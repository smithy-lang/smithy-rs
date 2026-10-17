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

pub(super) struct BoundHandler<B> {
    pub(super) operation: &'static OperationSchema<'static>,
    pub(super) collection_config: RequestBodyCollectionConfig,
    pub(super) handler_route: SyncRoute<crate::body::RequestBody<B>>,
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
#[derive(Debug)]
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
    /// The served protocols in priority order. Every configuration checks claims;
    /// a single metadata router can do so without an async routing loop.
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

/// The response when no protocol claims the request or offers a deferred rejection.
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
    B: http_body::Body<Data = Bytes> + Send + Unpin + 'static,
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
        request.extensions_mut().insert(SelectedOperation::new(
            self.state.protocols[protocol].protocol.clone(),
            binding.operation,
            binding.collection_config,
        ));
        binding.handler_route.clone().call_owned(request)
    }

    fn route_request(&self, request: Request<crate::body::RequestBody<B>>) -> MultiProtocolRoutingFuture<B> {
        let state = match &self.state.protocols[..] {
            // A single metadata router can check its claim synchronously.
            [ProtocolAndRouter {
                router: SharedProtocolRouter::Metadata(router),
                ..
            }] => self.route(router.as_ref(), request),
            _ => State::Routing {
                // TODO: Investigate avoiding this Arc clone when routing needs no body I/O.
                future: Box::pin(self.clone().route_protocols(request)),
            },
        };
        MultiProtocolRoutingFuture { inner: state }
    }

    /// Checks the service's only metadata protocol without allocating a routing future.
    fn route(&self, router: &dyn MetadataProtocolRouter, request: Request<crate::body::RequestBody<B>>) -> State<B> {
        // Probe with the head only: the parts move over and back, nothing is cloned,
        // and the router stays free of the transport body type.
        let (parts, body) = request.into_parts();
        let probe = Request::from_parts(parts, ());
        let selected = match router.claim(&probe) {
            RouteClaim::ClaimedWithRoute(selected) => Ok(selected),
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

    /// Tries protocols in canonical order, retaining the collected body if a router declines.
    async fn route_protocols(
        self,
        request: Request<crate::body::RequestBody<B>>,
    ) -> Result<Response<BoxBody>, Infallible> {
        let (parts, mut body) = request.into_parts();
        let mut probe = Request::from_parts(parts, ());
        let mut collected: Option<Bytes> = None;
        let mut streaming = None;
        let mut deferred = None;

        for (index, protocol) in self.state.protocols.iter().enumerate() {
            let selected = match &protocol.router {
                SharedProtocolRouter::Metadata(router) => match router.claim(&probe) {
                    RouteClaim::ClaimedWithRoute(selected) => Ok(selected),
                    RouteClaim::Claimed => router.route(&probe),
                    RouteClaim::DeferredRejection(error) => {
                        deferred.get_or_insert((index, error));
                        continue;
                    }
                    RouteClaim::NoClaim => continue,
                },
                SharedProtocolRouter::Body(router) => {
                    let claim = router.claim(&probe);
                    match claim {
                        BodyRouteClaim::ClaimedWithRoute(selected) => Ok(selected),
                        BodyRouteClaim::NoClaim => continue,
                        BodyRouteClaim::DeferredRejection(error) => {
                            deferred.get_or_insert((index, error));
                            continue;
                        }
                        BodyRouteClaim::NeedsBodyToClaim | BodyRouteClaim::Claimed => {
                            // Skip body-dependent claims for recognized streaming inputs.
                            // An ownership claim already made from the head retains priority.
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
                                RouteClaim::DeferredRejection(error) => {
                                    deferred.get_or_insert((index, error));
                                    None
                                }
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
        Ok(match deferred {
            Some((index, error)) => self.reject(index, error),
            None => unclaimed(),
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
    B: http_body::Body<Data = Bytes> + Send + Unpin + 'static,
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

impl<B> MultiProtocolRoutingService<B> {
    /// Starts configuring routing for the supplied service schema.
    pub fn builder(service: &'static ServiceSchema<'static>) -> MultiProtocolRoutingServiceBuilder<B> {
        MultiProtocolRoutingServiceBuilder::new(service)
    }
}
/// Any compatible body enters. The transport body `B` and an already-normalized
/// [`RequestBody<B>`](crate::body::RequestBody) stay unerased; any other body — tests, adapters,
/// upgrade layers — is erased into a boxed state (see [`RequestBody::new`](crate::body::RequestBody::new)).
impl<B, RB> Service<Request<RB>> for MultiProtocolRoutingService<B>
where
    B: http_body::Body<Data = Bytes> + Send + Unpin + 'static,
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

#[cfg(test)]
mod tests {
    use super::super::test_helpers::{binding, FIRST, REST_JSON, SECOND};
    use super::*;
    use crate::body::Body;
    use http::{HeaderValue, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn send_only_transport_bodies_preserve_shared_routing() {
        use crate::body::{BoxBody, RequestBody};
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

        fn route() -> SyncRoute<RequestBody<BoxBody>> {
            SyncRoute::new(tower::service_fn(|request: Request<RequestBody<BoxBody>>| async {
                assert!(request.extensions().get::<SelectedOperation>().is_some());
                let bytes = request.into_body().collect().await.unwrap().to_bytes();
                Ok::<_, Infallible>(Response::new(crate::body::to_boxed(bytes)))
            }))
        }

        // Sharing handler services does not require their request bodies to be Sync.
        crate::test_helpers::assert_send::<MultiProtocolRoutingService<BoxBody>>();
        crate::test_helpers::assert_sync::<MultiProtocolRoutingService<BoxBody>>();
        crate::test_helpers::assert_sync::<SyncRoute<RequestBody<BoxBody>>>();

        for schema in [&REST_JSON, &MULTI] {
            let service = MultiProtocolRoutingServiceBuilder::<BoxBody>::new(schema)
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
                .body(RequestBody::<BoxBody>::from_bytes(Bytes::from_static(b"normalized")))
                .unwrap();

            // Exercise public ingress and the Send-only normalized body concurrently,
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
                assert!(request.extensions().get::<SelectedOperation>().is_some());
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
