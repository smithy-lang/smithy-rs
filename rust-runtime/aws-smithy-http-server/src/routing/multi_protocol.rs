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
use tower::{Layer, Service};

use crate::{body::BoxBody, protocol::ProtocolShape, response::IntoResponse, routing::Router};

// ============================================================================
// SelectedProtocol (request extension)
// ============================================================================

/// The Smithy shape ID selected for a request. [`ProtocolService`] inserts it
/// into request extensions after detection and before dispatch or unknown-operation
/// handling. Middleware and request extractors can read it through
/// [`Request::extensions`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SelectedProtocol(pub &'static str);

// ============================================================================
// DetectionResult
// ============================================================================

/// The result of protocol detection.
///
/// `Matched` selects the protocol and supplies its matched route. `Detected`
/// selects the protocol and asks [`ProtocolService`] to resolve the route.
/// Returning `None` from [`ProtocolDetector::detect`] delegates to the inner service.
pub enum DetectionResult<S> {
    /// The selected protocol's matched route.
    Matched(S),
    /// The protocol was selected; its router must resolve the route.
    Detected,
}

// ============================================================================
// ProtocolDetector trait
// ============================================================================

/// Determines whether this detector selects its protocol for a request.
///
/// Returning `None` delegates to the inner service. `Detected` selects this
/// protocol and lets [`ProtocolService`] resolve the route; a failed lookup
/// produces the protocol-specific error via the router's `IntoResponse` impl.
/// `Matched` supplies the selected route directly.
///
/// # Borrowing constraint
///
/// Detection receives the request by shared reference, so it cannot pass the
/// original request directly to `Service::call`, which requires ownership.
pub trait ProtocolDetector<B, S> {
    /// The absolute Smithy shape ID of this protocol.
    fn protocol_id(&self) -> &'static str;

    /// Detect whether this protocol should handle the request.
    ///
    /// The `router` is provided for protocols that pre-match the route and return
    /// `Matched(route)`. Other protocols can ignore it and return `Detected`.
    fn detect(&self, req: &Request<B>, router: &impl Router<B, Service = S>) -> Option<DetectionResult<S>>;
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

    fn call(&mut self, _req: Request<B>) -> Self::Future {
        std::future::ready(Ok(Response::builder()
            .status(http::StatusCode::NOT_FOUND)
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(crate::body::to_boxed("{}"))
            .expect("valid response")))
    }
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
/// - If `detect` returns `Matched(route)` → dispatch directly.
/// - If `detect` returns `Detected` → call `router.match_route()`, dispatch or error.
/// - If `detect` returns `None` → delegate to `inner` (next protocol).
///
/// Once a protocol claims a request (`Detected` or `Matched`), the request is NOT
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
    P: ProtocolDetector<B, S>,
    R: Router<B, Service = S>,
    R::Error: IntoResponse<P>,
    S: Service<Request<B>, Response = Response<RespBody>, Error = E>,
    S::Future: Future<Output = Result<Response<RespBody>, E>>,
    Inner: Service<Request<B>, Response = Response<RespBody>, Error = E>,
    Inner::Future: Future<Output = Result<Response<RespBody>, E>>,
    RespBody: From<BoxBody>,
{
    type Response = Response<RespBody>;
    type Error = E;
    type Future = ProtocolServiceFuture<S::Future, Inner::Future, RespBody, E>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut req: Request<B>) -> Self::Future {
        match self.protocol.detect(&req, &self.router) {
            Some(DetectionResult::Matched(mut route)) => {
                // Fast path: protocol detected AND route pre-matched.
                let id = self.protocol.protocol_id();
                tracing::debug!(protocol = %id, "multi-protocol: matched (pre-resolved route)");
                req.extensions_mut().insert(SelectedProtocol(id));
                ProtocolServiceFuture::Route {
                    future: route.call(req),
                }
            }
            Some(DetectionResult::Detected) => {
                // Protocol claims this request. Do route lookup.
                let id = self.protocol.protocol_id();
                tracing::debug!(protocol = %id, "multi-protocol: detected, resolving route");
                req.extensions_mut().insert(SelectedProtocol(id));
                match self.router.match_route(&req) {
                    Ok(mut route) => ProtocolServiceFuture::Route {
                        future: route.call(req),
                    },
                    Err(error) => {
                        // Protocol owns this request but operation is unknown.
                        // Use the router's protocol-specific error response — NEVER fall through.
                        tracing::debug!(protocol = %id, "multi-protocol: unknown operation for detected protocol");
                        let rejection = error.into_response().map(RespBody::from);
                        ProtocolServiceFuture::Rejection {
                            response: Some(Ok(rejection)),
                        }
                    }
                }
            }
            None => {
                // Not this protocol, try next.
                ProtocolServiceFuture::Inner {
                    future: self.inner.call(req),
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
/// They return `Detected` (not `Matched`) — the routing service resolves the route.
/// If the route doesn't exist, it's an "unknown operation" error for this protocol,
/// NOT a fall-through to the next protocol.
macro_rules! impl_header_detection_protocol {
    ($marker:ty, $detect:expr) => {
        impl<B, S: Clone> ProtocolDetector<B, S> for $marker {
            fn protocol_id(&self) -> &'static str {
                <$marker as ProtocolShape>::ID.absolute()
            }

            #[inline]
            fn detect(&self, req: &Request<B>, _router: &impl Router<B, Service = S>) -> Option<DetectionResult<S>> {
                let detect: fn(&Request<B>) -> bool = $detect;
                if detect(req) {
                    Some(DetectionResult::Detected)
                } else {
                    None
                }
            }
        }
    };
}

impl_header_detection_protocol!(
    crate::protocol::aws_json_11::AwsJson1_1,
    is_aws_json_11
);
impl_header_detection_protocol!(
    crate::protocol::aws_json_10::AwsJson1_0,
    is_aws_json_10
);
impl_header_detection_protocol!(
    crate::protocol::rpc_v2_cbor::RpcV2Cbor,
    is_rpc_v2_cbor
);

/// Macro for route-matching protocols.
///
/// These protocols check content-type first (cheap), then attempt route matching.
/// They return `Matched(route)` on success — avoiding a double route lookup.
/// They return `None` if content-type doesn't match OR if no route matches,
/// because content-type alone is not enough to definitively claim a request
/// (e.g., any JSON POST could look like RestJson1).
macro_rules! impl_route_matching_protocol {
    ($marker:ty, $content_type_check:expr) => {
        impl<B, S: Clone> ProtocolDetector<B, S> for $marker {
            fn protocol_id(&self) -> &'static str {
                <$marker as ProtocolShape>::ID.absolute()
            }

            #[inline]
            fn detect(&self, req: &Request<B>, router: &impl Router<B, Service = S>) -> Option<DetectionResult<S>> {
                let check: fn(&Request<B>) -> bool = $content_type_check;
                if check(req) {
                    router.match_route(req).ok().map(DetectionResult::Matched)
                } else {
                    None
                }
            }
        }
    };
}

impl_route_matching_protocol!(
    crate::protocol::rest_json_1::RestJson1,
    is_json_content_type
);
impl_route_matching_protocol!(
    crate::protocol::rest_xml::RestXml,
    is_xml_content_type
);

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

/// Check if Content-Type indicates JSON (for RestJson1).
/// Also accepts event stream content type since RestJson1 can use event streams.
/// Accepts: `application/json`, any `*/*+json` structured syntax suffix, and
/// `application/vnd.amazon.eventstream`.
/// A missing Content-Type defaults to true (GET requests, payloadless operations).
fn is_json_content_type<B>(req: &Request<B>) -> bool {
    req.headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            let essence = v.split(';').next().unwrap_or("").trim();
            essence.eq_ignore_ascii_case("application/json")
                || essence.ends_with("+json")
                || essence.eq_ignore_ascii_case("application/vnd.amazon.eventstream")
        })
        .unwrap_or(true) // Default to true if no content-type (GET requests, etc.)
}

/// Check if Content-Type indicates XML (for RestXml).
/// Accepts: `application/xml`, `text/xml`, and any `*/*+xml` structured syntax suffix.
fn is_xml_content_type<B>(req: &Request<B>) -> bool {
    req.headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            let essence = v.split(';').next().unwrap_or("").trim();
            essence.eq_ignore_ascii_case("application/xml")
                || essence.eq_ignore_ascii_case("text/xml")
                || essence.ends_with("+xml")
        })
        .unwrap_or(false)
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
    fn test_json_content_type_detection() {
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/foo")
            .header("content-type", "application/json")
            .body(())
            .unwrap();
        assert!(is_json_content_type(&req));

        let req_no_ct = Request::builder()
            .method(http::Method::GET)
            .uri("/foo")
            .body(())
            .unwrap();
        assert!(is_json_content_type(&req_no_ct));
    }

    #[test]
    fn test_xml_content_type_detection() {
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/foo")
            .header("content-type", "application/xml")
            .body(())
            .unwrap();
        assert!(is_xml_content_type(&req));
        assert!(!is_json_content_type(&req));
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
        let a = SelectedProtocol("aws.protocols#restJson1");
        let b = a;
        assert_eq!(a, b);
        assert_eq!(a.0, "aws.protocols#restJson1");
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
        assert!(matches!(result, Some(DetectionResult::Detected)));
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

        // Protocol still claims it — returns Detected (not None).
        // Route resolution failure will be handled by ProtocolService as a protocol error.
        let result = AwsJson1_1.detect(&req, &router);
        assert!(matches!(result, Some(DetectionResult::Detected)));
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
            Some(DetectionResult::Matched(route)) => assert_eq!(route, "handler"),
            _ => panic!("expected Matched"),
        }
    }

    #[test]
    fn test_route_matching_protocol_returns_none_on_no_route() {
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

        // Correct content-type but unknown path
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/unknown-op")
            .header("content-type", "application/json")
            .body(())
            .unwrap();

        // Returns None — content-type alone doesn't claim ownership for REST protocols
        let result = RestJson1.detect(&req, &router);
        assert!(result.is_none());
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

        // Protocol claims it (Detected) — even though SomeOp won't be found in the router,
        // the protocol still says "this is mine". ProtocolService will produce an error
        // response rather than falling through to the next protocol.
        let result = RpcV2Cbor.detect(&req, &router);
        assert!(matches!(result, Some(DetectionResult::Detected)));
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

    /// End-to-end test: when RpcV2Cbor claims a request (Detected) but the operation
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

    #[test]
    fn test_json_content_type_rejects_unrelated_contains() {
        // A content-type like "text/not-really-json" must NOT match
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/foo")
            .header("content-type", "text/not-really-json")
            .body(())
            .unwrap();
        assert!(!is_json_content_type(&req));
    }

    #[test]
    fn test_json_content_type_accepts_structured_suffix() {
        // "application/vnd.api+json" should match via +json suffix
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/foo")
            .header("content-type", "application/vnd.api+json")
            .body(())
            .unwrap();
        assert!(is_json_content_type(&req));
    }

    #[test]
    fn test_xml_content_type_rejects_unrelated_contains() {
        // "text/not-really-xml-at-all" must NOT match
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/foo")
            .header("content-type", "text/not-really-xml-at-all")
            .body(())
            .unwrap();
        assert!(!is_xml_content_type(&req));
    }

    #[test]
    fn test_xml_content_type_accepts_structured_suffix() {
        // "application/soap+xml" should match via +xml suffix
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/foo")
            .header("content-type", "application/soap+xml")
            .body(())
            .unwrap();
        assert!(is_xml_content_type(&req));
    }
}
