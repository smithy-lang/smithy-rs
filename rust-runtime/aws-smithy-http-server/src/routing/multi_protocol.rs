/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Multi-protocol routing support for Smithy services.
//!
//! A single service can support several Smithy protocols simultaneously (e.g.
//! RestJson1, RpcV2Cbor). Each protocol is installed with a [`ProtocolLayer`],
//! producing a statically nested [`ProtocolService`] stack. A `ProtocolService`
//! delegates to its inner service only when its detector returns `None`. Generated
//! stacks terminate in [`DefaultNotFoundService`].
//!
//! Layer nesting defines detection order: the outermost [`ProtocolService`] is
//! checked first.
//!
//! [`ProtocolLayer`] accepts any type implementing [`ProtocolDetector`], including
//! protocols defined in downstream crates.

use std::{
    convert::Infallible,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use http::{Request, Response};
use pin_project_lite::pin_project;
use tower::{util::Oneshot, Layer, Service, ServiceExt};

use crate::{body::BoxBody, protocol::ProtocolShape, response::IntoResponse, routing::Router, shape_id::ShapeId};

// ============================================================================
// SelectedProtocol (request extension)
// ============================================================================

/// The Smithy shape ID selected for a request. [`ProtocolService`] inserts it
/// into request extensions after detection and before dispatch or unknown-operation
/// handling. Middleware and request extractors can read it through
/// [`Request::extensions`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SelectedProtocol(pub ShapeId);

// ============================================================================
// ProtocolClaim
// ============================================================================

/// The result of protocol claiming.
///
/// `RouteMatched` selects the protocol and supplies its matched route. `Claimed`
/// selects the protocol and asks [`ProtocolService`] to resolve the route.
/// `NoClaim` delegates to the inner service.
pub enum ProtocolClaim<S, R> {
    /// This protocol has no claim on the request.
    NoClaim,
    /// The selected protocol's matched route.
    RouteMatched(S),
    /// The protocol was selected; its router must resolve the route.
    Claimed,
    /// This protocol rejected the request exclusively.
    Rejected(R),
    /// This protocol rejected the request, but a later protocol may still claim it.
    RejectedNonExclusive(R),
}

/// Provides route specificity and a protocol-specific response for non-exclusive fallback rejections.
pub trait FallbackRejection<P> {
    fn route_rank(&self) -> usize;

    fn response_factory(&self) -> fn() -> Response<BoxBody>;
}

impl<P> FallbackRejection<P> for Infallible {
    fn route_rank(&self) -> usize {
        match *self {}
    }

    fn response_factory(&self) -> fn() -> Response<BoxBody> {
        match *self {}
    }
}

// ============================================================================
// ProtocolDetector trait
// ============================================================================

/// Determines whether this detector selects its protocol for a request.
///
/// Returning `NoClaim` delegates to the inner service. `Claimed` selects this
/// protocol and lets [`ProtocolService`] resolve the route; a failed lookup
/// produces the protocol-specific error via the router's `IntoResponse` impl.
/// `RouteMatched` supplies the selected route directly.
///
/// # Borrowing constraint
///
/// Detection receives the request by shared reference, so it cannot pass the
/// original request directly to `Service::call`, which requires ownership.
pub trait ProtocolDetector<B, R>
where
    R: Router<B>,
{
    type Rejection;

    /// The absolute Smithy shape ID of this protocol.
    fn protocol_id(&self) -> ShapeId;

    /// Detect whether this protocol should handle the request.
    ///
    /// The `router` is provided for protocols that pre-match the route and return
    /// `RouteMatched(route)`. Other protocols can ignore it and return `Claimed`.
    fn detect(&self, req: &Request<B>, router: &R) -> ProtocolClaim<R::Service, Self::Rejection>;
}

// ============================================================================
// DefaultNotFoundService (terminal service)
// ============================================================================

/// Terminal service that returns `404 Not Found` when every detector delegates.
#[derive(Debug, Clone, Copy)]
pub struct DefaultNotFoundService;

impl<B> Service<Request<B>> for DefaultNotFoundService {
    type Response = Response<BoxBody>;
    type Error = Infallible;
    type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut req: Request<B>) -> Self::Future {
        if let Some(response) = take_best_fallback_response(req.extensions_mut()) {
            return std::future::ready(Ok(response));
        }

        std::future::ready(Ok(Response::builder()
            .status(http::StatusCode::NOT_FOUND)
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(crate::body::to_boxed("{}"))
            .expect("valid response")))
    }
}

#[derive(Clone)]
struct StoredFallbackRejections {
    rejections: Vec<StoredFallbackRejection>,
}

#[derive(Clone)]
struct StoredFallbackRejection {
    route_rank: usize,
    response_factory: fn() -> Response<BoxBody>,
}

fn record_fallback_response(
    extensions: &mut http::Extensions,
    route_rank: usize,
    response_factory: fn() -> Response<BoxBody>,
) {
    let stored = extensions
        .get_mut::<StoredFallbackRejections>()
        .map(|stored| &mut stored.rejections);
    match stored {
        Some(rejections) => rejections.push(StoredFallbackRejection {
            route_rank,
            response_factory,
        }),
        None => {
            extensions.insert(StoredFallbackRejections {
                rejections: vec![StoredFallbackRejection {
                    route_rank,
                    response_factory,
                }],
            });
        }
    }
}

fn clear_fallback_responses(extensions: &mut http::Extensions) {
    extensions.remove::<StoredFallbackRejections>();
}

fn take_best_fallback_response(extensions: &mut http::Extensions) -> Option<Response<BoxBody>> {
    let stored = extensions.remove::<StoredFallbackRejections>()?;
    let best_index = stored
        .rejections
        .iter()
        .enumerate()
        .fold(None, |best: Option<(usize, usize)>, (index, rejection)| match best {
            Some((best_index, best_rank)) if best_rank >= rejection.route_rank => Some((best_index, best_rank)),
            _ => Some((index, rejection.route_rank)),
        })
        .map(|(index, _rank)| index)?;
    Some((stored.rejections.into_iter().nth(best_index)?.response_factory)())
}

// ============================================================================
// ProtocolLayer / ProtocolService
// ============================================================================

/// A Tower layer that adds one protocol-routing service in front of an inner service.
///
/// Holds a protocol marker (implements [`ProtocolDetector`]) and a router (implements [`Router`]).
/// Nest layers in protocol detection order, with the highest-priority protocol outermost.
#[derive(Clone, Debug)]
pub struct ProtocolLayer<P, R> {
    protocol: P,
    router: R,
}

impl<P, R> ProtocolLayer<P, R> {
    /// Creates a layer for the given protocol and router.
    pub fn new(protocol: P, router: R) -> Self {
        Self { protocol, router }
    }
}

impl<P, R, Inner> Layer<Inner> for ProtocolLayer<P, R>
where
    P: Clone,
    R: Clone,
{
    type Service = ProtocolService<P, R, Inner>;

    fn layer(&self, inner: Inner) -> Self::Service {
        ProtocolService {
            protocol: self.protocol.clone(),
            router: self.router.clone(),
            inner,
        }
    }
}

/// The service produced by [`ProtocolLayer`].
///
/// Checks `protocol` first:
/// - If `detect` returns `RouteMatched(route)` → dispatch directly.
/// - If `detect` returns `Claimed` → call `router.match_route()`, dispatch or error.
/// - If `detect` returns `NoClaim` → delegate to `inner` (next protocol).
///
/// Once a protocol claims a request (`Claimed` or `RouteMatched`), the request is NOT
/// handed to the next protocol even if route lookup fails. This ensures that a
/// request definitively owned by one protocol (e.g., RpcV2Cbor with the correct
/// `smithy-protocol` header) gets a proper protocol-specific error rather than
/// being misrouted to a different protocol.
#[derive(Clone, Debug)]
pub struct ProtocolService<P, R, Inner> {
    protocol: P,
    router: R,
    inner: Inner,
}

impl<B, P, R, S, Inner, RespBody, E> Service<Request<B>> for ProtocolService<P, R, Inner>
where
    P: ProtocolDetector<B, R>,
    R: Router<B, Service = S>,
    R::Error: IntoResponse<P>,
    P::Rejection: IntoResponse<P> + FallbackRejection<P>,
    S: Service<Request<B>, Response = Response<RespBody>, Error = E>,
    Inner: Service<Request<B>, Response = Response<RespBody>, Error = E> + Clone,
    RespBody: From<BoxBody>,
{
    type Response = Response<RespBody>;
    type Error = E;
    type Future = ProtocolServiceFuture<Oneshot<S, Request<B>>, Oneshot<Inner, Request<B>>, RespBody, E>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut req: Request<B>) -> Self::Future {
        match self.protocol.detect(&req, &self.router) {
            ProtocolClaim::RouteMatched(route) => {
                // Fast path: protocol detected AND route pre-matched.
                let id = self.protocol.protocol_id();
                tracing::debug!(protocol = id.absolute(), "multi-protocol: matched (pre-resolved route)");
                clear_fallback_responses(req.extensions_mut());
                req.extensions_mut().insert(SelectedProtocol(id));
                ProtocolServiceFuture::Route {
                    future: route.oneshot(req),
                }
            }
            ProtocolClaim::Claimed => {
                // Protocol claims this request. Do route lookup.
                let id = self.protocol.protocol_id();
                tracing::debug!(protocol = id.absolute(), "multi-protocol: detected, resolving route");
                clear_fallback_responses(req.extensions_mut());
                req.extensions_mut().insert(SelectedProtocol(id));
                match self.router.match_route(&req) {
                    Ok(route) => ProtocolServiceFuture::Route {
                        future: route.oneshot(req),
                    },
                    Err(error) => {
                        // Protocol owns this request but operation is unknown.
                        // Use the router's protocol-specific error response — NEVER fall through.
                        tracing::debug!(
                            protocol = id.absolute(),
                            "multi-protocol: unknown operation for detected protocol"
                        );
                        let rejection = error.into_response().map(RespBody::from);
                        ProtocolServiceFuture::Rejection {
                            response: Some(Ok(rejection)),
                        }
                    }
                }
            }
            ProtocolClaim::Rejected(rejection) => {
                clear_fallback_responses(req.extensions_mut());
                let response = rejection.into_response().map(RespBody::from);
                ProtocolServiceFuture::Rejection {
                    response: Some(Ok(response)),
                }
            }
            ProtocolClaim::RejectedNonExclusive(rejection) => {
                let route_rank = rejection.route_rank();
                let response_factory = rejection.response_factory();
                record_fallback_response(req.extensions_mut(), route_rank, response_factory);
                ProtocolServiceFuture::Inner {
                    future: self.inner.clone().oneshot(req),
                }
            }
            ProtocolClaim::NoClaim => {
                // Not this protocol, try next.
                ProtocolServiceFuture::Inner {
                    future: self.inner.clone().oneshot(req),
                }
            }
        }
    }
}

// ============================================================================
// ProtocolServiceFuture
// ============================================================================

pin_project! {
    /// A three-variant future for [`ProtocolService`].
    ///
    /// - `Route` — a protocol claimed the request and resolved the route; dispatch it.
    /// - `Rejection` — a protocol claimed the request but route lookup failed; return
    ///   the protocol-specific error response immediately (no fall-through).
    /// - `Inner` — this protocol did not claim the request; delegate to the next one.
    #[doc(hidden)]
    #[project = ProtocolServiceFutureProj]
    pub enum ProtocolServiceFuture<RouteFut, InnerFut, RespBody, E> {
        Route { #[pin] future: RouteFut },
        Rejection { response: Option<Result<Response<RespBody>, E>> },
        Inner { #[pin] future: InnerFut },
    }
}

impl<RouteFut, InnerFut, RespBody, E> Future for ProtocolServiceFuture<RouteFut, InnerFut, RespBody, E>
where
    RouteFut: Future<Output = Result<Response<RespBody>, E>>,
    InnerFut: Future<Output = Result<Response<RespBody>, E>>,
{
    type Output = Result<Response<RespBody>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            ProtocolServiceFutureProj::Route { future } => future.poll(cx),
            ProtocolServiceFutureProj::Rejection { response } => Poll::Ready(
                response
                    .take()
                    .expect("ProtocolServiceFuture::Rejection polled after completion"),
            ),
            ProtocolServiceFutureProj::Inner { future } => future.poll(cx),
        }
    }
}

// ============================================================================
// ProtocolDetector impls for public protocol markers
// ============================================================================

/// Macro for header-detected protocols.
///
/// These protocols can definitively claim ownership from headers alone.
/// They return `Claimed` (not `RouteMatched`) — the routing service resolves the route.
/// If the route doesn't exist, it's an "unknown operation" error for this protocol,
/// NOT a fall-through to the next protocol.
macro_rules! impl_header_detection_protocol {
    ($marker:ty, $detect:expr) => {
        impl<B, R> ProtocolDetector<B, R> for $marker
        where
            R: Router<B>,
        {
            type Rejection = Infallible;

            fn protocol_id(&self) -> ShapeId {
                <$marker as ProtocolShape>::ID
            }

            #[inline]
            fn detect(&self, req: &Request<B>, _router: &R) -> ProtocolClaim<R::Service, Self::Rejection> {
                let detect: fn(&Request<B>) -> bool = $detect;
                if detect(req) {
                    ProtocolClaim::Claimed
                } else {
                    ProtocolClaim::NoClaim
                }
            }
        }
    };
}

impl_header_detection_protocol!(crate::protocol::aws_json_11::AwsJson1_1, is_aws_json_11);
impl_header_detection_protocol!(crate::protocol::aws_json_10::AwsJson1_0, is_aws_json_10);
impl_header_detection_protocol!(crate::protocol::rpc_v2_cbor::RpcV2Cbor, is_rpc_v2_cbor);

/// Macro for REST protocols.
///
/// REST protocols claim through route metadata, including operation-specific
/// request `Content-Type` policy.
macro_rules! impl_rest_protocol {
    ($marker:ty) => {
        impl<B, S: Clone> ProtocolDetector<B, crate::protocol::rest::router::RestRouter<S>> for $marker {
            type Rejection = crate::protocol::rest::router::RestProtocolRejection;

            fn protocol_id(&self) -> ShapeId {
                <$marker as ProtocolShape>::ID
            }

            #[inline]
            fn detect(
                &self,
                req: &Request<B>,
                router: &crate::protocol::rest::router::RestRouter<S>,
            ) -> ProtocolClaim<S, Self::Rejection> {
                match router.claim_route(req) {
                    crate::protocol::rest::router::RestRouteClaim::NoClaim => ProtocolClaim::NoClaim,
                    crate::protocol::rest::router::RestRouteClaim::RouteMatched { route, .. } => {
                        ProtocolClaim::RouteMatched(route)
                    }
                    crate::protocol::rest::router::RestRouteClaim::RejectedNonExclusive { route_rank, reason } => {
                        ProtocolClaim::RejectedNonExclusive(crate::protocol::rest::router::RestProtocolRejection {
                            route_rank,
                            reason,
                        })
                    }
                }
            }
        }
    };
}

impl_rest_protocol!(crate::protocol::rest_json_1::RestJson1);
impl_rest_protocol!(crate::protocol::rest_xml::RestXml);

// ============================================================================
// Protocol detection functions
// ============================================================================

/// Check if request has the `smithy-protocol: rpc-v2-cbor` header.
fn is_rpc_v2_cbor<B>(req: &Request<B>) -> bool {
    req.headers()
        .get("smithy-protocol")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == "rpc-v2-cbor")
        .unwrap_or(false)
}

/// Check if request has AWS JSON markers (x-amz-target header).
fn has_aws_json_target<B>(req: &Request<B>) -> bool {
    req.headers().contains_key("x-amz-target")
}

/// Check if Content-Type indicates AWS JSON 1.0.
/// Uses exact media-type matching (ignoring parameters like charset).
fn is_aws_json_10_content_type<B>(req: &Request<B>) -> bool {
    req.headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            // Extract the essence (type/subtype) ignoring parameters like ";charset=utf-8"
            let essence = v.split(';').next().unwrap_or("").trim();
            essence.eq_ignore_ascii_case("application/x-amz-json-1.0")
        })
        .unwrap_or(false)
}

/// Check if Content-Type indicates AWS JSON 1.1.
/// Uses exact media-type matching (ignoring parameters like charset).
fn is_aws_json_11_content_type<B>(req: &Request<B>) -> bool {
    req.headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            let essence = v.split(';').next().unwrap_or("").trim();
            essence.eq_ignore_ascii_case("application/x-amz-json-1.1")
        })
        .unwrap_or(false)
}

/// Combined check for AWS JSON 1.0: has target header AND correct content-type.
fn is_aws_json_10<B>(req: &Request<B>) -> bool {
    has_aws_json_target(req) && is_aws_json_10_content_type(req)
}

/// Combined check for AWS JSON 1.1: has target header AND correct content-type.
fn is_aws_json_11<B>(req: &Request<B>) -> bool {
    has_aws_json_target(req) && is_aws_json_11_content_type(req)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::aws_json::router::AwsJsonRouter;
    use crate::protocol::rest::router::RestRouter;
    use crate::protocol::rpc_v2_cbor::router::RpcV2CborRouter;

    #[test]
    fn test_rpc_v2_detection() {
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/service/MyService/operation/MyOp")
            .header("smithy-protocol", "rpc-v2-cbor")
            .body(())
            .unwrap();
        assert!(is_rpc_v2_cbor(&req));

        let req_no_header = Request::builder()
            .method(http::Method::POST)
            .uri("/service/MyService/operation/MyOp")
            .body(())
            .unwrap();
        assert!(!is_rpc_v2_cbor(&req_no_header));
    }

    #[test]
    fn test_aws_json_11_detection() {
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/")
            .header("x-amz-target", "MyService.MyOp")
            .header("content-type", "application/x-amz-json-1.1")
            .body(())
            .unwrap();
        assert!(has_aws_json_target(&req));
        assert!(is_aws_json_11_content_type(&req));
        assert!(!is_aws_json_10_content_type(&req));
    }

    #[test]
    fn test_aws_json_10_detection() {
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/")
            .header("x-amz-target", "MyService.MyOp")
            .header("content-type", "application/x-amz-json-1.0")
            .body(())
            .unwrap();
        assert!(has_aws_json_target(&req));
        assert!(is_aws_json_10_content_type(&req));
        assert!(!is_aws_json_11_content_type(&req));
    }

    #[test]
    fn test_default_not_found_service() {
        let mut service = DefaultNotFoundService;
        let waker = futures_util::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        assert!(matches!(
            Service::<Request<()>>::poll_ready(&mut service, &mut cx),
            Poll::Ready(Ok(()))
        ));
    }

    #[test]
    fn test_selected_protocol_is_extension_friendly() {
        use crate::protocol::{rest_json_1::RestJson1, ProtocolShape};

        let a = SelectedProtocol(RestJson1::ID);
        let b = a;
        assert_eq!(a, b);
        assert_eq!(a.0.absolute(), "aws.protocols#restJson1");
        assert_eq!(a.0.namespace(), "aws.protocols");
        assert_eq!(a.0.name(), "restJson1");
    }

    #[test]
    fn test_header_detected_protocol_returns_detected_not_matched() {
        use crate::protocol::aws_json_11::AwsJson1_1;

        let router: AwsJsonRouter<&str> = vec![("MyService.MyOp", "handler")].into_iter().collect();

        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/")
            .header("x-amz-target", "MyService.MyOp")
            .header("content-type", "application/x-amz-json-1.1")
            .body(())
            .unwrap();

        let result = AwsJson1_1.detect(&req, &router);
        assert!(matches!(result, ProtocolClaim::Claimed));
    }

    #[test]
    fn test_header_detected_protocol_claims_even_unknown_operation() {
        use crate::protocol::aws_json_11::AwsJson1_1;

        let router: AwsJsonRouter<&str> = vec![("MyService.KnownOp", "handler")].into_iter().collect();

        // Request targets an unknown operation, but headers match AwsJson1.1
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/")
            .header("x-amz-target", "MyService.UnknownOp")
            .header("content-type", "application/x-amz-json-1.1")
            .body(())
            .unwrap();

        // Protocol still claims it — returns Claimed.
        // Route resolution failure will be handled by ProtocolService as a protocol error.
        let result = AwsJson1_1.detect(&req, &router);
        assert!(matches!(result, ProtocolClaim::Claimed));
    }

    #[test]
    fn test_route_matching_protocol_returns_matched_with_route() {
        use crate::protocol::rest_json_1::RestJson1;
        use crate::routing::request_spec::{PathAndQuerySpec, PathSpec, QuerySpec, RequestSpec, UriSpec};

        let spec = RequestSpec::new(
            http::Method::POST,
            UriSpec::new(PathAndQuerySpec::new(
                PathSpec::from_vector_unchecked(vec![crate::routing::request_spec::PathSegment::Literal(
                    String::from("my-op"),
                )]),
                QuerySpec::from_vector_unchecked(vec![]),
            )),
        );
        let router: RestRouter<&str> = vec![(spec, "handler")].into_iter().collect();

        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/my-op")
            .header("content-type", "application/json")
            .body(())
            .unwrap();

        let result = RestJson1.detect(&req, &router);
        match result {
            ProtocolClaim::RouteMatched(route) => assert_eq!(route, "handler"),
            _ => panic!("expected RouteMatched"),
        }
    }

    #[test]
    fn test_route_matching_protocol_claims_explicit_content_type_on_unknown_route() {
        use crate::protocol::rest_json_1::RestJson1;
        use crate::routing::request_spec::{PathAndQuerySpec, PathSpec, QuerySpec, RequestSpec, UriSpec};

        let spec = RequestSpec::new(
            http::Method::POST,
            UriSpec::new(PathAndQuerySpec::new(
                PathSpec::from_vector_unchecked(vec![crate::routing::request_spec::PathSegment::Literal(
                    String::from("known-op"),
                )]),
                QuerySpec::from_vector_unchecked(vec![]),
            )),
        );
        let router: RestRouter<&str> = vec![(spec, "handler")].into_iter().collect();

        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/unknown-op")
            .header("content-type", "application/json")
            .body(())
            .unwrap();

        // REST claims only when the path/query route shape participates.
        let result = RestJson1.detect(&req, &router);
        assert!(matches!(result, ProtocolClaim::NoClaim));
    }

    #[test]
    fn test_rpc_v2_cbor_claims_request_does_not_fall_through() {
        use crate::protocol::rpc_v2_cbor::RpcV2Cbor;

        let router: RpcV2CborRouter<&str> = vec![("MyService.UnknownOp", "handler")].into_iter().collect();

        // Valid RpcV2Cbor request (correct header)
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/service/MyService/operation/SomeOp")
            .header("smithy-protocol", "rpc-v2-cbor")
            .body(())
            .unwrap();

        // Protocol claims it (Claimed) — even though SomeOp won't be found in the router,
        // the protocol still says "this is mine". ProtocolService will produce an error
        // response rather than falling through to the next protocol.
        let result = RpcV2Cbor.detect(&req, &router);
        assert!(matches!(result, ProtocolClaim::Claimed));
    }

    #[test]
    fn test_protocol_layer_builds() {
        use crate::protocol::rest_json_1::RestJson1;
        use crate::protocol::rpc_v2_cbor::RpcV2Cbor;
        use crate::routing::request_spec::RequestSpec;

        let rest_router: RestRouter<()> = Vec::<(RequestSpec, ())>::new().into_iter().collect();
        let cbor_router: RpcV2CborRouter<()> = Vec::<(&'static str, ())>::new().into_iter().collect();

        let _service = ProtocolLayer::new(RpcV2Cbor, cbor_router)
            .layer(ProtocolLayer::new(RestJson1, rest_router).layer(DefaultNotFoundService));
    }

    /// End-to-end test: when RpcV2Cbor claims a request but the operation
    /// does not exist in its router, ProtocolService returns a protocol-specific
    /// rejection (404 with `application/cbor`) and NEVER calls the inner service.
    ///
    /// This is the Smithy protocol-identification compliance rule: once a protocol
    /// claims ownership, an unknown operation is terminal for that protocol.
    #[tokio::test]
    async fn test_detected_protocol_never_falls_through_on_unknown_operation() {
        use crate::protocol::rpc_v2_cbor::RpcV2Cbor;
        use crate::routing::Route;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        // A service that panics if called — proves inner is never invoked.
        #[derive(Clone)]
        struct PanicService(Arc<AtomicBool>);

        impl Service<Request<crate::body::BoxBody>> for PanicService {
            type Response = Response<BoxBody>;
            type Error = Infallible;
            type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

            fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
                Poll::Ready(Ok(()))
            }

            fn call(&mut self, _req: Request<crate::body::BoxBody>) -> Self::Future {
                self.0.store(true, Ordering::SeqCst);
                panic!("Inner service must NEVER be called when protocol claims the request");
            }
        }

        // Router with one known operation — but we'll send an unknown one.
        let router: RpcV2CborRouter<Route<crate::body::BoxBody>> =
            Vec::<(&'static str, Route<crate::body::BoxBody>)>::new()
                .into_iter()
                .collect();

        let inner_called = Arc::new(AtomicBool::new(false));
        let inner = PanicService(inner_called.clone());

        let mut service = ProtocolLayer::new(RpcV2Cbor, router).layer(inner);

        // RpcV2Cbor-claimed request to an unknown operation.
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/service/MyService/operation/NonExistentOp")
            .header("smithy-protocol", "rpc-v2-cbor")
            .body(crate::body::BoxBody::default())
            .unwrap();

        let response = Service::call(&mut service, req).await.unwrap();

        // Verify: protocol-specific rejection, not a generic 404 from inner.
        assert_eq!(response.status(), http::StatusCode::NOT_FOUND);
        assert_eq!(
            response.headers().get(http::header::CONTENT_TYPE).unwrap(),
            "application/cbor"
        );
        // Inner was never called.
        assert!(!inner_called.load(Ordering::SeqCst));
    }

    /// End-to-end test: when AwsJson1_1 claims a request but the operation is unknown,
    /// ProtocolService returns a protocol-specific 404 and never calls inner.
    #[tokio::test]
    async fn test_aws_json_11_never_falls_through_on_unknown_operation() {
        use crate::protocol::aws_json_11::AwsJson1_1;
        use crate::routing::Route;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        #[derive(Clone)]
        struct PanicService(Arc<AtomicBool>);

        impl Service<Request<crate::body::BoxBody>> for PanicService {
            type Response = Response<BoxBody>;
            type Error = Infallible;
            type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

            fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
                Poll::Ready(Ok(()))
            }

            fn call(&mut self, _req: Request<crate::body::BoxBody>) -> Self::Future {
                self.0.store(true, Ordering::SeqCst);
                panic!("Inner service must NEVER be called when protocol claims the request");
            }
        }

        let router: AwsJsonRouter<Route<crate::body::BoxBody>> =
            Vec::<(&'static str, Route<crate::body::BoxBody>)>::new()
                .into_iter()
                .collect();

        let inner_called = Arc::new(AtomicBool::new(false));
        let inner = PanicService(inner_called.clone());

        let mut service = ProtocolLayer::new(AwsJson1_1, router).layer(inner);

        // AwsJson1.1-claimed request to an unknown operation.
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/")
            .header("x-amz-target", "MyService.UnknownOp")
            .header("content-type", "application/x-amz-json-1.1")
            .body(crate::body::BoxBody::default())
            .unwrap();

        let response = Service::call(&mut service, req).await.unwrap();

        assert_eq!(response.status(), http::StatusCode::NOT_FOUND);
        assert_eq!(
            response.headers().get(http::header::CONTENT_TYPE).unwrap(),
            "application/x-amz-json-1.1"
        );
        assert!(!inner_called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn protocol_routes_are_polled_ready_before_call() {
        use crate::protocol::rest::router::{RequestContentType, RestRouteSpec};
        use crate::protocol::rest_json_1::RestJson1;
        use crate::routing::request_spec::{PathAndQuerySpec, PathSegment, PathSpec, QuerySpec, RequestSpec, UriSpec};
        use crate::routing::Route;

        #[derive(Clone)]
        struct ReadyRequired {
            ready: bool,
        }

        impl Service<Request<BoxBody>> for ReadyRequired {
            type Response = Response<BoxBody>;
            type Error = Infallible;
            type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

            fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
                self.ready = true;
                Poll::Ready(Ok(()))
            }

            fn call(&mut self, _req: Request<BoxBody>) -> Self::Future {
                assert!(self.ready, "route called without first being polled ready");
                std::future::ready(Ok(Response::new(BoxBody::default())))
            }
        }

        let spec = RequestSpec::new(
            http::Method::POST,
            UriSpec::new(PathAndQuerySpec::new(
                PathSpec::from_vector_unchecked(vec![PathSegment::Literal(String::from("ready"))]),
                QuerySpec::from_vector_unchecked(vec![]),
            )),
        );
        let route_spec = RestRouteSpec::new(spec, RequestContentType::Expected("application/json"));
        let router: RestRouter<Route<BoxBody>> = vec![(route_spec, Route::new(ReadyRequired { ready: false }))]
            .into_iter()
            .collect();
        let mut service = ProtocolLayer::new(RestJson1, router).layer(DefaultNotFoundService);
        let request = Request::builder()
            .method(http::Method::POST)
            .uri("/ready")
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(BoxBody::default())
            .unwrap();

        Service::call(&mut service, request).await.unwrap();
    }

    #[tokio::test]
    async fn ambiguous_rest_request_uses_detection_order_and_sets_selected_protocol() {
        use crate::protocol::rest::router::{RequestContentType, RestRouteSpec};
        use crate::protocol::{rest_json_1::RestJson1, rest_xml::RestXml};
        use crate::routing::request_spec::{PathAndQuerySpec, PathSegment, PathSpec, QuerySpec, RequestSpec, UriSpec};
        use crate::routing::Route;

        #[derive(Clone)]
        struct AssertSelected(&'static str);

        impl Service<Request<BoxBody>> for AssertSelected {
            type Response = Response<BoxBody>;
            type Error = Infallible;
            type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

            fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
                Poll::Ready(Ok(()))
            }

            fn call(&mut self, req: Request<BoxBody>) -> Self::Future {
                assert_eq!(
                    req.extensions()
                        .get::<SelectedProtocol>()
                        .map(|selected| selected.0.absolute()),
                    Some(self.0)
                );
                std::future::ready(Ok(Response::new(BoxBody::default())))
            }
        }

        fn router(expected_protocol: &'static str) -> RestRouter<Route<BoxBody>> {
            let spec = RequestSpec::new(
                http::Method::GET,
                UriSpec::new(PathAndQuerySpec::new(
                    PathSpec::from_vector_unchecked(vec![PathSegment::Literal(String::from("ambiguous"))]),
                    QuerySpec::from_vector_unchecked(vec![]),
                )),
            );
            let route_spec =
                RestRouteSpec::new(spec, RequestContentType::Expected("application/vnd.amazon.eventstream"));
            vec![(route_spec, Route::new(AssertSelected(expected_protocol)))]
                .into_iter()
                .collect()
        }

        let mut xml_first = ProtocolLayer::new(RestXml, router(<RestXml as ProtocolShape>::ID.absolute())).layer(
            ProtocolLayer::new(RestJson1, router(<RestJson1 as ProtocolShape>::ID.absolute()))
                .layer(DefaultNotFoundService),
        );
        let request = Request::builder()
            .method(http::Method::GET)
            .uri("/ambiguous")
            .header(http::header::CONTENT_TYPE, "application/vnd.amazon.eventstream")
            .body(BoxBody::default())
            .unwrap();
        Service::call(&mut xml_first, request).await.unwrap();

        let mut json_first = ProtocolLayer::new(RestJson1, router(<RestJson1 as ProtocolShape>::ID.absolute())).layer(
            ProtocolLayer::new(RestXml, router(<RestXml as ProtocolShape>::ID.absolute()))
                .layer(DefaultNotFoundService),
        );
        let request = Request::builder()
            .method(http::Method::GET)
            .uri("/ambiguous")
            .header(http::header::CONTENT_TYPE, "application/vnd.amazon.eventstream")
            .body(BoxBody::default())
            .unwrap();
        Service::call(&mut json_first, request).await.unwrap();
    }

    #[tokio::test]
    async fn terminal_fallback_returns_highest_ranked_rest_rejection() {
        use crate::protocol::rest::router::{RequestContentType, RestRouteSpec};
        use crate::protocol::{rest_json_1::RestJson1, rest_xml::RestXml};
        use crate::routing::request_spec::{
            PathAndQuerySpec, PathSegment, PathSpec, QuerySegment, QuerySpec, RequestSpec, UriSpec,
        };
        use crate::routing::Route;

        #[derive(Clone)]
        struct Unused;

        impl Service<Request<BoxBody>> for Unused {
            type Response = Response<BoxBody>;
            type Error = Infallible;
            type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

            fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
                Poll::Ready(Ok(()))
            }

            fn call(&mut self, _req: Request<BoxBody>) -> Self::Future {
                panic!("wrong content type must not dispatch")
            }
        }

        fn spec(query_segments: Vec<QuerySegment>, expected: &'static str) -> RestRouteSpec {
            RestRouteSpec::new(
                RequestSpec::new(
                    http::Method::POST,
                    UriSpec::new(PathAndQuerySpec::new(
                        PathSpec::from_vector_unchecked(vec![PathSegment::Literal(String::from("items"))]),
                        QuerySpec::from_vector_unchecked(query_segments),
                    )),
                ),
                RequestContentType::Expected(expected),
            )
        }

        let xml_router: RestRouter<Route<BoxBody>> = vec![(spec(Vec::new(), "application/xml"), Route::new(Unused))]
            .into_iter()
            .collect();
        let json_router: RestRouter<Route<BoxBody>> = vec![(
            spec(
                vec![QuerySegment::KeyValue(String::from("mode"), String::from("test"))],
                "application/json",
            ),
            Route::new(Unused),
        )]
        .into_iter()
        .collect();

        let mut service = ProtocolLayer::new(RestXml, xml_router)
            .layer(ProtocolLayer::new(RestJson1, json_router).layer(DefaultNotFoundService));
        let request = Request::builder()
            .method(http::Method::POST)
            .uri("/items?mode=test")
            .header(http::header::CONTENT_TYPE, "text/plain")
            .body(BoxBody::default())
            .unwrap();

        let response = Service::call(&mut service, request).await.unwrap();
        assert_eq!(response.status(), http::StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(
            response.headers().get(http::header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }

    #[tokio::test]
    async fn terminal_fallback_uses_first_recorded_rejection_for_rank_ties() {
        use crate::protocol::rest::router::{RequestContentType, RestRouteSpec};
        use crate::protocol::{rest_json_1::RestJson1, rest_xml::RestXml};
        use crate::routing::request_spec::{PathAndQuerySpec, PathSegment, PathSpec, QuerySpec, RequestSpec, UriSpec};
        use crate::routing::Route;

        #[derive(Clone)]
        struct Unused;

        impl Service<Request<BoxBody>> for Unused {
            type Response = Response<BoxBody>;
            type Error = Infallible;
            type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

            fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
                Poll::Ready(Ok(()))
            }

            fn call(&mut self, _req: Request<BoxBody>) -> Self::Future {
                panic!("wrong content type must not dispatch")
            }
        }

        fn spec(expected: &'static str) -> RestRouteSpec {
            RestRouteSpec::new(
                RequestSpec::new(
                    http::Method::POST,
                    UriSpec::new(PathAndQuerySpec::new(
                        PathSpec::from_vector_unchecked(vec![PathSegment::Literal(String::from("items"))]),
                        QuerySpec::from_vector_unchecked(vec![]),
                    )),
                ),
                RequestContentType::Expected(expected),
            )
        }

        let xml_router: RestRouter<Route<BoxBody>> = vec![(spec("application/xml"), Route::new(Unused))]
            .into_iter()
            .collect();
        let json_router: RestRouter<Route<BoxBody>> = vec![(spec("application/json"), Route::new(Unused))]
            .into_iter()
            .collect();

        let mut service = ProtocolLayer::new(RestXml, xml_router)
            .layer(ProtocolLayer::new(RestJson1, json_router).layer(DefaultNotFoundService));
        let request = Request::builder()
            .method(http::Method::POST)
            .uri("/items")
            .header(http::header::CONTENT_TYPE, "text/plain")
            .body(BoxBody::default())
            .unwrap();

        let response = Service::call(&mut service, request).await.unwrap();
        assert_eq!(response.status(), http::StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(
            response.headers().get(http::header::CONTENT_TYPE).unwrap(),
            "application/xml"
        );
    }

    #[tokio::test]
    async fn rest_fallback_is_cleared_when_later_protocol_claims() {
        use crate::protocol::rest::router::{RequestContentType, RestRouteSpec};
        use crate::protocol::{rest_json_1::RestJson1, rest_xml::RestXml};
        use crate::routing::request_spec::{PathAndQuerySpec, PathSegment, PathSpec, QuerySpec, RequestSpec, UriSpec};
        use crate::routing::Route;

        #[derive(Clone)]
        struct JsonHandler;

        impl Service<Request<BoxBody>> for JsonHandler {
            type Response = Response<BoxBody>;
            type Error = Infallible;
            type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

            fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
                Poll::Ready(Ok(()))
            }

            fn call(&mut self, _req: Request<BoxBody>) -> Self::Future {
                std::future::ready(Ok(Response::builder()
                    .status(http::StatusCode::NO_CONTENT)
                    .body(BoxBody::default())
                    .unwrap()))
            }
        }

        #[derive(Clone)]
        struct Unused;

        impl Service<Request<BoxBody>> for Unused {
            type Response = Response<BoxBody>;
            type Error = Infallible;
            type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

            fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
                Poll::Ready(Ok(()))
            }

            fn call(&mut self, _req: Request<BoxBody>) -> Self::Future {
                panic!("xml must reject non-exclusively")
            }
        }

        fn spec(expected: &'static str) -> RestRouteSpec {
            RestRouteSpec::new(
                RequestSpec::new(
                    http::Method::POST,
                    UriSpec::new(PathAndQuerySpec::new(
                        PathSpec::from_vector_unchecked(vec![PathSegment::Literal(String::from("items"))]),
                        QuerySpec::from_vector_unchecked(vec![]),
                    )),
                ),
                RequestContentType::Expected(expected),
            )
        }

        let xml_router: RestRouter<Route<BoxBody>> = vec![(spec("application/xml"), Route::new(Unused))]
            .into_iter()
            .collect();
        let json_router: RestRouter<Route<BoxBody>> = vec![(spec("application/json"), Route::new(JsonHandler))]
            .into_iter()
            .collect();

        let mut service = ProtocolLayer::new(RestXml, xml_router)
            .layer(ProtocolLayer::new(RestJson1, json_router).layer(DefaultNotFoundService));
        let request = Request::builder()
            .method(http::Method::POST)
            .uri("/items")
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(BoxBody::default())
            .unwrap();

        let response = Service::call(&mut service, request).await.unwrap();
        assert_eq!(response.status(), http::StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn outer_middleware_wraps_multi_protocol_success_and_fallback() {
        use crate::protocol::rest::router::{RequestContentType, RestRouteSpec};
        use crate::protocol::{rest_json_1::RestJson1, rest_xml::RestXml};
        use crate::routing::request_spec::{PathAndQuerySpec, PathSegment, PathSpec, QuerySpec, RequestSpec, UriSpec};
        use crate::routing::Route;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        #[derive(Clone)]
        struct CountLayer(Arc<AtomicUsize>);

        impl<S> Layer<S> for CountLayer {
            type Service = CountService<S>;

            fn layer(&self, inner: S) -> Self::Service {
                CountService {
                    inner,
                    calls: self.0.clone(),
                }
            }
        }

        #[derive(Clone)]
        struct CountService<S> {
            inner: S,
            calls: Arc<AtomicUsize>,
        }

        impl<S> Service<Request<BoxBody>> for CountService<S>
        where
            S: Service<Request<BoxBody>, Response = Response<BoxBody>, Error = Infallible> + Send + 'static,
            S::Future: Send + 'static,
        {
            type Response = Response<BoxBody>;
            type Error = Infallible;
            type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

            fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
                self.inner.poll_ready(cx)
            }

            fn call(&mut self, req: Request<BoxBody>) -> Self::Future {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Box::pin(self.inner.call(req))
            }
        }

        #[derive(Clone)]
        struct JsonHandler;

        impl Service<Request<BoxBody>> for JsonHandler {
            type Response = Response<BoxBody>;
            type Error = Infallible;
            type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

            fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
                Poll::Ready(Ok(()))
            }

            fn call(&mut self, _req: Request<BoxBody>) -> Self::Future {
                std::future::ready(Ok(Response::builder()
                    .status(http::StatusCode::NO_CONTENT)
                    .body(BoxBody::default())
                    .unwrap()))
            }
        }

        #[derive(Clone)]
        struct Unused;

        impl Service<Request<BoxBody>> for Unused {
            type Response = Response<BoxBody>;
            type Error = Infallible;
            type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

            fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
                Poll::Ready(Ok(()))
            }

            fn call(&mut self, _req: Request<BoxBody>) -> Self::Future {
                panic!("xml must reject non-exclusively")
            }
        }

        fn spec(expected: &'static str) -> RestRouteSpec {
            RestRouteSpec::new(
                RequestSpec::new(
                    http::Method::POST,
                    UriSpec::new(PathAndQuerySpec::new(
                        PathSpec::from_vector_unchecked(vec![PathSegment::Literal(String::from("items"))]),
                        QuerySpec::from_vector_unchecked(vec![]),
                    )),
                ),
                RequestContentType::Expected(expected),
            )
        }

        let xml_router: RestRouter<Route<BoxBody>> = vec![(spec("application/xml"), Route::new(Unused))]
            .into_iter()
            .collect();
        let json_router: RestRouter<Route<BoxBody>> = vec![(spec("application/json"), Route::new(JsonHandler))]
            .into_iter()
            .collect();
        let inner = ProtocolLayer::new(RestXml, xml_router)
            .layer(ProtocolLayer::new(RestJson1, json_router).layer(DefaultNotFoundService));

        let calls = Arc::new(AtomicUsize::new(0));
        let mut service = CountLayer(calls.clone()).layer(inner);

        let request = Request::builder()
            .method(http::Method::POST)
            .uri("/items")
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(BoxBody::default())
            .unwrap();
        let response = Service::call(&mut service, request).await.unwrap();
        assert_eq!(response.status(), http::StatusCode::NO_CONTENT);

        let request = Request::builder()
            .method(http::Method::POST)
            .uri("/items")
            .header(http::header::CONTENT_TYPE, "text/plain")
            .body(BoxBody::default())
            .unwrap();
        let response = Service::call(&mut service, request).await.unwrap();
        assert_eq!(response.status(), http::StatusCode::UNSUPPORTED_MEDIA_TYPE);

        let request = Request::builder()
            .method(http::Method::GET)
            .uri("/missing")
            .body(BoxBody::default())
            .unwrap();
        let response = Service::call(&mut service, request).await.unwrap();
        assert_eq!(response.status(), http::StatusCode::NOT_FOUND);

        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    // ======================================================================
    // Media-type edge-case tests
    // ======================================================================

    #[test]
    fn test_aws_json_11_rejects_similar_content_type() {
        // "application/x-amz-json-1.10" should NOT match 1.1
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/")
            .header("x-amz-target", "MyService.MyOp")
            .header("content-type", "application/x-amz-json-1.10")
            .body(())
            .unwrap();
        assert!(!is_aws_json_11(&req));
        assert!(!is_aws_json_10(&req));
    }

    #[test]
    fn test_aws_json_10_rejects_similar_content_type() {
        // "application/x-amz-json-1.00" should NOT match 1.0
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/")
            .header("x-amz-target", "MyService.MyOp")
            .header("content-type", "application/x-amz-json-1.00")
            .body(())
            .unwrap();
        assert!(!is_aws_json_10(&req));
    }

    #[test]
    fn test_aws_json_11_accepts_with_charset_parameter() {
        // "application/x-amz-json-1.1; charset=utf-8" should match (parameters are ignored)
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/")
            .header("x-amz-target", "MyService.MyOp")
            .header("content-type", "application/x-amz-json-1.1; charset=utf-8")
            .body(())
            .unwrap();
        assert!(is_aws_json_11(&req));
    }
}
