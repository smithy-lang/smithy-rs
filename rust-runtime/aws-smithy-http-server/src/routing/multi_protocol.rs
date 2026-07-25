/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Multi-protocol routing support for Smithy services.
//!
//! A single service can support several Smithy protocols simultaneously (e.g.
//! RestJson1, RpcV2Cbor). Each protocol is installed with a [`ProtocolLayer`],
//! producing a statically nested [`ProtocolService`] stack. A service checks its
//! protocol and delegates misses to the inner service. The stack terminates in a
//! user-supplied service, with [`DefaultNotFoundService`] providing the generated
//! default.
//!
//! Protocols are checked from the outermost service inward. Generated servers
//! determine that order during code generation from the built-in protocol order
//! and decorator-provided relative constraints.
//!
//! [`ProtocolLayer`] is open: any type implementing [`ProtocolSlot`] can be
//! installed, including protocols defined in downstream crates. Nothing in this
//! crate needs to know about those protocols or assign them numeric priorities.

use std::{
    convert::Infallible,
    future::Future,
    task::{Context, Poll},
};

use futures_util::future::Either;

use http::{Request, Response};
use tower::{Layer, Service};

use crate::{
    body::BoxBody,
    protocol::{
        aws_json::router::AwsJsonRouter, rest::router::RestRouter, rpc_v2_cbor::router::RpcV2CborRouter, ProtocolShape,
    },
    routing::{Router, RoutingService},
};

// ============================================================================
// SelectedProtocol (request extension)
// ============================================================================

/// The Smithy [Shape ID](https://smithy.io/2.0/spec/model.html#shape-id) of the
/// protocol that handled a request, inserted into request extensions by
/// [`ProtocolService`] before dispatching.
///
/// Handlers can read it to determine which protocol was selected:
///
/// ```ignore
/// fn handler(req: http::Request<Body>) {
///     if let Some(SelectedProtocol(id)) = req.extensions().get::<SelectedProtocol>() {
///         println!("handled by {id}"); // e.g. "aws.protocols#restJson1"
///     }
/// }
/// ```
///
/// The value is the protocol's absolute shape ID string (e.g.
/// `"aws.protocols#restJson1"`), so it works uniformly for public and
/// downstream/internal protocols without a closed enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SelectedProtocol(pub &'static str);

// ============================================================================
// ProtocolSlot (zero-cost protocol detection)
// ============================================================================

/// A protocol slot: detects whether it can handle a request, and if so, handles it.
///
/// The `Match` associated type carries any work done during detection through to
/// [`call_matched`](ProtocolSlot::call_matched), avoiding recomputation. For cheap
/// header-only detection it is `()`; for protocols that must match a route it
/// caches the matched service.
pub trait ProtocolSlot<B, RespBody, E> {
    /// The future returned by [`call_matched`](ProtocolSlot::call_matched).
    type Future: Future<Output = Result<Response<RespBody>, E>>;

    /// Proof the request can be handled, carried from detection into handling.
    type Match;

    /// The absolute Smithy shape ID of this protocol. This is inserted into
    /// request extensions as [`SelectedProtocol`] when the slot handles a request.
    fn protocol_id(&self) -> &'static str;

    /// Returns `Some` with the match proof if this protocol can handle `req`.
    fn can_handle(&self, req: &Request<B>) -> Option<Self::Match>;

    /// Handles the request using the proof from [`can_handle`](ProtocolSlot::can_handle).
    fn call_matched(&mut self, req: Request<B>, matched: Self::Match) -> Self::Future;
}

// ============================================================================
// Routing service type aliases
// ============================================================================

/// Type alias for RpcV2Cbor routing service.
pub type CborRoutingService<S> = RoutingService<RpcV2CborRouter<S>, crate::protocol::rpc_v2_cbor::RpcV2Cbor>;
/// Type alias for AwsJson1.1 routing service.
pub type AwsJson11RoutingService<S> = RoutingService<AwsJsonRouter<S>, crate::protocol::aws_json_11::AwsJson1_1>;
/// Type alias for AwsJson1.0 routing service.
pub type AwsJson10RoutingService<S> = RoutingService<AwsJsonRouter<S>, crate::protocol::aws_json_10::AwsJson1_0>;
/// Type alias for RestJson1 routing service.
pub type RestJson1RoutingService<S> = RoutingService<RestRouter<S>, crate::protocol::rest_json_1::RestJson1>;
/// Type alias for RestXml routing service.
pub type RestXmlRoutingService<S> = RoutingService<RestRouter<S>, crate::protocol::rest_xml::RestXml>;

// ============================================================================
// DefaultNotFoundService (default fallback)
// ============================================================================

/// Default fallback that returns `404 Not Found` when no protocol matches.
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

/// A Tower layer that adds one protocol-routing service in front of an inner
/// service.
///
/// Nest layers in protocol detection order, with the highest-priority protocol
/// outermost. The resulting [`ProtocolService`] checks its protocol and delegates
/// requests it cannot handle to the inner service.
#[derive(Clone, Debug)]
pub struct ProtocolLayer<P> {
    protocol: P,
}

impl<P> ProtocolLayer<P> {
    /// Creates a layer for `protocol`.
    pub fn new(protocol: P) -> Self {
        Self { protocol }
    }
}

impl<P, Inner> Layer<Inner> for ProtocolLayer<P>
where
    P: Clone,
{
    type Service = ProtocolService<P, Inner>;

    fn layer(&self, inner: Inner) -> Self::Service {
        ProtocolService {
            protocol: self.protocol.clone(),
            inner,
        }
    }
}

/// The service produced by [`ProtocolLayer`].
///
/// It checks `protocol` first and delegates misses to `inner`.
#[derive(Clone, Debug)]
pub struct ProtocolService<P, Inner> {
    protocol: P,
    inner: Inner,
}

impl<B, P, Inner, RespBody, E> Service<Request<B>> for ProtocolService<P, Inner>
where
    P: ProtocolSlot<B, RespBody, E>,
    Inner: Service<Request<B>, Response = Response<RespBody>, Error = E>,
    Inner::Future: Future<Output = Result<Response<RespBody>, E>>,
{
    type Response = Response<RespBody>;
    type Error = E;
    type Future = Either<P::Future, Inner::Future>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut req: Request<B>) -> Self::Future {
        if let Some(matched) = self.protocol.can_handle(&req) {
            let id = self.protocol.protocol_id();
            tracing::debug!(protocol = %id, "multi-protocol routing: request matched protocol");
            req.extensions_mut().insert(SelectedProtocol(id));
            Either::Left(self.protocol.call_matched(req, matched))
        } else {
            // TODO: we haven't called ready on the inner service.
            Either::Right(self.inner.call(req))
        }
    }
}

// ============================================================================
// ProtocolSlot impls for the public routing services
// ============================================================================

fn routing_service_protocol_id<R, P: ProtocolShape>(_: &RoutingService<R, P>) -> &'static str {
    P::ID.absolute()
}

macro_rules! impl_header_detection_protocol {
    ($alias:ident, $can_handle:expr) => {
        impl<S, B, RespBody, E> ProtocolSlot<B, RespBody, E> for $alias<S>
        where
            $alias<S>: Service<Request<B>, Response = Response<RespBody>, Error = E>,
            <$alias<S> as Service<Request<B>>>::Future: Future<Output = Result<Response<RespBody>, E>>,
        {
            type Future = <$alias<S> as Service<Request<B>>>::Future;
            type Match = ();

            fn protocol_id(&self) -> &'static str {
                routing_service_protocol_id(self)
            }

            #[inline]
            fn can_handle(&self, req: &Request<B>) -> Option<Self::Match> {
                let can: fn(&Request<B>) -> bool = $can_handle;
                can(req).then_some(())
            }

            fn call_matched(&mut self, req: Request<B>, _matched: Self::Match) -> Self::Future {
                Service::call(self, req)
            }
        }
    };
}

impl_header_detection_protocol!(CborRoutingService, is_rpc_v2_cbor);
impl_header_detection_protocol!(AwsJson11RoutingService, is_aws_json_11);
impl_header_detection_protocol!(AwsJson10RoutingService, is_aws_json_10);

/// Macro for route-matching protocols that also check content-type.
/// Content-type is checked first (cheap) before route matching (expensive).
/// `Match` caches the matched route so it isn't recomputed in `call_matched`.
macro_rules! impl_route_matching_protocol {
    ($alias:ident, $content_type_check:expr) => {
        impl<S, B, RespBody, E> ProtocolSlot<B, RespBody, E> for $alias<S>
        where
            RestRouter<S>: Router<B, Service = S>,
            S: Clone + Service<Request<B>, Response = Response<RespBody>, Error = E>,
            <S as Service<Request<B>>>::Future: Future<Output = Result<Response<RespBody>, E>>,
        {
            type Future = <S as Service<Request<B>>>::Future;
            type Match = S;

            fn protocol_id(&self) -> &'static str {
                routing_service_protocol_id(self)
            }

            #[inline]
            fn can_handle(&self, req: &Request<B>) -> Option<Self::Match> {
                let check: fn(&Request<B>) -> bool = $content_type_check;
                if check(req) {
                    let matched = self.router().match_route(req).ok()?;
                    Some(matched)
                } else {
                    None
                }
            }

            fn call_matched(&mut self, req: Request<B>, mut matched: Self::Match) -> Self::Future {
                matched.call(req)
            }
        }
    };
}

impl_route_matching_protocol!(RestJson1RoutingService, is_json_content_type);
impl_route_matching_protocol!(RestXmlRoutingService, is_xml_content_type);

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
fn is_aws_json_10_content_type<B>(req: &Request<B>) -> bool {
    req.headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.starts_with("application/x-amz-json-1.0"))
        .unwrap_or(false)
}

/// Check if Content-Type indicates AWS JSON 1.1.
fn is_aws_json_11_content_type<B>(req: &Request<B>) -> bool {
    req.headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.starts_with("application/x-amz-json-1.1"))
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
fn is_json_content_type<B>(req: &Request<B>) -> bool {
    req.headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            v.contains("application/json") || v.contains("+json") || v.contains("application/vnd.amazon.eventstream")
        })
        .unwrap_or(true) // Default to true if no content-type (GET requests, etc.)
}

/// Check if Content-Type indicates XML (for RestXml).
fn is_xml_content_type<B>(req: &Request<B>) -> bool {
    req.headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("application/xml") || v.contains("text/xml") || v.contains("+xml"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // Copy + 'static so it can live in http extensions.
        let a = SelectedProtocol("aws.protocols#restJson1");
        let b = a;
        assert_eq!(a, b);
        assert_eq!(a.0, "aws.protocols#restJson1");
    }

    #[test]
    fn test_layers_build_with_direct_terminal_service() {
        use crate::routing::request_spec::RequestSpec;

        let rest_router: RestRouter<()> = Vec::<(RequestSpec, ())>::new().into_iter().collect();
        let cbor_router: RpcV2CborRouter<()> = Vec::<(&'static str, ())>::new().into_iter().collect();

        let _service = ProtocolLayer::new(CborRoutingService::new(cbor_router))
            .layer(ProtocolLayer::new(RestJson1RoutingService::new(rest_router)).layer(DefaultNotFoundService));
    }
}
