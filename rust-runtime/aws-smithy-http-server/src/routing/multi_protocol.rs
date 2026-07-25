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
//! [`Fallback`] wrapping a user-supplied fallback service (defaulting to
//! [`DefaultNotFoundService`]).
//!
//! # Ordering
//!
//! Each protocol declares a [`ProtocolMeta::PRIORITY`] (lower is checked first).
//! The outermost service runs first, so layers must be nested in ascending
//! priority order. A compile-time guard rejects a misordered stack. The concrete
//! public protocols are spaced by 1000 to leave room for additional (e.g.
//! internal) protocols to slot in between:
//!
//! | Protocol   | Priority | Detection                                   |
//! |------------|----------|---------------------------------------------|
//! | RpcV2Cbor  | 1000     | `smithy-protocol: rpc-v2-cbor` header       |
//! | AwsJson1.1 | 2000     | `x-amz-target` + `application/x-amz-json-1.1`|
//! | AwsJson1.0 | 3000     | `x-amz-target` + `application/x-amz-json-1.0`|
//! | RestJson1  | 4000     | content-type + route matching               |
//! | RestXml    | 5000     | content-type + route matching               |
//!
//! # Extensibility
//!
//! [`ProtocolLayer`] is open: any type implementing [`ProtocolSlot`] can be
//! installed, including protocols defined in downstream crates. Such a protocol
//! declares its own `PRIORITY` and is placed at the appropriate layer in the
//! stack; nothing in this crate needs to know about it.

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
// ProtocolMeta (non-generic protocol metadata)
// ============================================================================

/// Compile-time metadata for a protocol slot, independent of request and response
/// types. This lets [`ProtocolLayer`] validate ordering before those types are
/// known.
pub trait ProtocolMeta {
    /// Detection priority; lower is checked earlier in the layer stack. Public
    /// protocols use multiples of 1000 (see the module table) so downstream
    /// protocols can slot in between.
    const PRIORITY: u16;

    /// The absolute Smithy shape ID of this protocol (e.g.
    /// `"aws.protocols#restJson1"`). Inserted into request extensions as
    /// [`SelectedProtocol`] when this slot handles a request.
    fn protocol_id(&self) -> &'static str;
}

// ============================================================================
// ProtocolSlot (zero-cost protocol detection)
// ============================================================================

/// A protocol slot: detects whether it can handle a request, and if so, handles it.
///
/// The `Match` associated type carries any work done during detection through to
/// [`call`](ProtocolSlot::call), avoiding recomputation. For cheap header-only
/// detection it is `()`; for protocols that must match a route it caches the
/// matched service.
pub trait ProtocolSlot<B, RespBody, E>: ProtocolMeta {
    /// The future returned by [`call`](ProtocolSlot::call).
    type Future: Future<Output = Result<Response<RespBody>, E>>;

    /// Proof the request can be handled, carried from detection into handling.
    type Match;

    /// Returns `Some` with the match proof if this protocol can handle `req`.
    fn can_handle(&self, req: &Request<B>) -> Option<Self::Match>;

    /// Handles the request using the proof from [`can_handle`](ProtocolSlot::can_handle).
    fn call(&mut self, req: Request<B>, matched: Self::Match) -> Self::Future;
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
// ProtocolStack (layer-stack metadata)
// ============================================================================

/// Implemented by every valid protocol service stack: a [`Fallback`] terminal or
/// a [`ProtocolService`] wrapper. Exposes the outermost protocol priority so the
/// next [`ProtocolLayer`] can assert ascending priority order.
pub trait ProtocolStack {
    /// Priority of the outermost protocol in this stack; `u16::MAX` for a bare
    /// [`Fallback`] terminal.
    const HEAD_PRIORITY: u16;
}

// ============================================================================
// Fallback (layer-stack terminal)
// ============================================================================

/// Innermost service invoked when no protocol matches.
///
/// Wrapping the user's fallback gives the layer stack a typed end that
/// participates in the priority-order guard (`HEAD_PRIORITY = u16::MAX`).
#[derive(Debug, Clone, Copy)]
pub struct Fallback<F> {
    inner: F,
}

impl Fallback<DefaultNotFoundService> {
    /// Creates a terminal using [`DefaultNotFoundService`].
    pub fn not_found() -> Self {
        Fallback {
            inner: DefaultNotFoundService,
        }
    }
}

impl Default for Fallback<DefaultNotFoundService> {
    fn default() -> Self {
        Self::not_found()
    }
}

impl<F> Fallback<F> {
    /// Creates a terminal using a custom fallback service.
    pub fn new(inner: F) -> Self {
        Fallback { inner }
    }
}

impl<F> ProtocolStack for Fallback<F> {
    const HEAD_PRIORITY: u16 = u16::MAX;
}

impl<B, F, RespBody, E> Service<Request<B>> for Fallback<F>
where
    F: Service<Request<B>, Response = Response<RespBody>, Error = E>,
{
    type Response = Response<RespBody>;
    type Error = E;
    type Future = F::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        self.inner.call(req)
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
    P: Clone + ProtocolMeta,
    Inner: ProtocolStack,
{
    type Service = ProtocolService<P, Inner>;

    fn layer(&self, inner: Inner) -> Self::Service {
        let service = ProtocolService {
            protocol: self.protocol.clone(),
            inner,
        };
        let () = ProtocolService::<P, Inner>::_ORDER_OK;
        service
    }
}

/// The service produced by [`ProtocolLayer`].
///
/// It checks `protocol` first and delegates misses to `inner`. The compile-time
/// ordering guard rejects a layer stack whose protocols are not in ascending
/// priority order.
#[derive(Clone, Debug)]
pub struct ProtocolService<P, Inner> {
    protocol: P,
    inner: Inner,
}

// Compile-time order guard. Evaluating this const fails the build if an outer
// protocol has a lower selection priority than the inner stack.
impl<P: ProtocolMeta, Inner: ProtocolStack> ProtocolService<P, Inner> {
    const _ORDER_OK: () = assert!(
        P::PRIORITY <= Inner::HEAD_PRIORITY,
        "protocol layers are out of priority order: a higher-priority protocol is placed inside a lower-priority protocol",
    );
}

impl<P: ProtocolMeta, Inner: ProtocolStack> ProtocolStack for ProtocolService<P, Inner> {
    const HEAD_PRIORITY: u16 = P::PRIORITY;
}

impl<B, P, Inner, RespBody, E> Service<Request<B>> for ProtocolService<P, Inner>
where
    P: ProtocolSlot<B, RespBody, E>,
    Inner: ProtocolStack + Service<Request<B>, Response = Response<RespBody>, Error = E>,
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
            Either::Left(self.protocol.call(req, matched))
        } else {
            Either::Right(self.inner.call(req))
        }
    }
}

// ============================================================================
// ProtocolSlot / ProtocolMeta impls for the public routing services
// ============================================================================

macro_rules! impl_header_detected_slot {
    ($alias:ident, $protocol:path, $priority:literal, $can_handle:expr) => {
        impl<S> ProtocolMeta for $alias<S> {
            const PRIORITY: u16 = $priority;
            fn protocol_id(&self) -> &'static str {
                <$protocol as ProtocolShape>::ID.absolute()
            }
        }

        impl<S, B, RespBody, E> ProtocolSlot<B, RespBody, E> for $alias<S>
        where
            $alias<S>: Service<Request<B>, Response = Response<RespBody>, Error = E>,
            <$alias<S> as Service<Request<B>>>::Future: Future<Output = Result<Response<RespBody>, E>>,
        {
            type Future = <$alias<S> as Service<Request<B>>>::Future;
            type Match = ();

            #[inline]
            fn can_handle(&self, req: &Request<B>) -> Option<Self::Match> {
                let can: fn(&Request<B>) -> bool = $can_handle;
                can(req).then_some(())
            }

            fn call(&mut self, req: Request<B>, _matched: Self::Match) -> Self::Future {
                Service::call(self, req)
            }
        }
    };
}

impl_header_detected_slot!(
    CborRoutingService,
    crate::protocol::rpc_v2_cbor::RpcV2Cbor,
    1000,
    is_rpc_v2_cbor
);
impl_header_detected_slot!(
    AwsJson11RoutingService,
    crate::protocol::aws_json_11::AwsJson1_1,
    2000,
    |req| { has_aws_json_target(req) && is_aws_json_11_content_type(req) }
);
impl_header_detected_slot!(
    AwsJson10RoutingService,
    crate::protocol::aws_json_10::AwsJson1_0,
    3000,
    |req| { has_aws_json_target(req) && is_aws_json_10_content_type(req) }
);

/// RestJson1 - content-type + route matching (expensive). `Match` caches the
/// matched route so it isn't recomputed in `call`.
impl<S> ProtocolMeta for RestJson1RoutingService<S> {
    const PRIORITY: u16 = 4000;
    fn protocol_id(&self) -> &'static str {
        <crate::protocol::rest_json_1::RestJson1 as ProtocolShape>::ID.absolute()
    }
}

impl<S, B, RespBody, E> ProtocolSlot<B, RespBody, E> for RestJson1RoutingService<S>
where
    RestRouter<S>: Router<B, Service = S>,
    S: Clone + Service<Request<B>, Response = Response<RespBody>, Error = E>,
    <S as Service<Request<B>>>::Future: Future<Output = Result<Response<RespBody>, E>>,
{
    type Future = <S as Service<Request<B>>>::Future;
    type Match = S;

    #[inline]
    fn can_handle(&self, req: &Request<B>) -> Option<Self::Match> {
        let matched = self.router().match_route(req).ok()?;
        if is_json_content_type(req) {
            Some(matched)
        } else {
            None
        }
    }

    fn call(&mut self, req: Request<B>, mut matched: Self::Match) -> Self::Future {
        matched.call(req)
    }
}

/// RestXml - content-type + route matching (expensive). `Match` caches the
/// matched route so it isn't recomputed in `call`.
impl<S> ProtocolMeta for RestXmlRoutingService<S> {
    const PRIORITY: u16 = 5000;
    fn protocol_id(&self) -> &'static str {
        <crate::protocol::rest_xml::RestXml as ProtocolShape>::ID.absolute()
    }
}

impl<S, B, RespBody, E> ProtocolSlot<B, RespBody, E> for RestXmlRoutingService<S>
where
    RestRouter<S>: Router<B, Service = S>,
    S: Clone + Service<Request<B>, Response = Response<RespBody>, Error = E>,
    <S as Service<Request<B>>>::Future: Future<Output = Result<Response<RespBody>, E>>,
{
    type Future = <S as Service<Request<B>>>::Future;
    type Match = S;

    #[inline]
    fn can_handle(&self, req: &Request<B>) -> Option<Self::Match> {
        let matched = self.router().match_route(req).ok()?;
        if is_xml_content_type(req) {
            Some(matched)
        } else {
            None
        }
    }

    fn call(&mut self, req: Request<B>, mut matched: Self::Match) -> Self::Future {
        matched.call(req)
    }
}

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
    fn test_fallback_terminal_head_priority_is_max() {
        assert_eq!(
            <Fallback<DefaultNotFoundService> as ProtocolStack>::HEAD_PRIORITY,
            u16::MAX
        );
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
    fn test_public_protocol_priorities_are_spaced_by_1000() {
        assert_eq!(<CborRoutingService<()> as ProtocolMeta>::PRIORITY, 1000);
        assert_eq!(<AwsJson11RoutingService<()> as ProtocolMeta>::PRIORITY, 2000);
        assert_eq!(<AwsJson10RoutingService<()> as ProtocolMeta>::PRIORITY, 3000);
        assert_eq!(<RestJson1RoutingService<()> as ProtocolMeta>::PRIORITY, 4000);
        assert_eq!(<RestXmlRoutingService<()> as ProtocolMeta>::PRIORITY, 5000);
    }

    #[test]
    fn test_layers_build_in_priority_order() {
        use crate::routing::request_spec::RequestSpec;

        let rest_router: RestRouter<()> = Vec::<(RequestSpec, ())>::new().into_iter().collect();
        let cbor_router: RpcV2CborRouter<()> = Vec::<(&'static str, ())>::new().into_iter().collect();

        let service = ProtocolLayer::new(CborRoutingService::new(cbor_router))
            .layer(ProtocolLayer::new(RestJson1RoutingService::new(rest_router)).layer(Fallback::not_found()));

        // The outer service priority is RpcV2Cbor's (1000), read off the concrete type.
        fn head_priority<T: ProtocolStack>(_: &T) -> u16 {
            T::HEAD_PRIORITY
        }
        assert_eq!(head_priority(&service), 1000);
    }
}
