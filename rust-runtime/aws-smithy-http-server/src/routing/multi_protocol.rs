/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Multi-protocol routing support for Smithy services.
//!
//! A single service can support several Smithy protocols simultaneously (e.g.
//! RestJson1, RpcV2Cbor). Protocols are composed into a [`ProtocolChain`]: each
//! link holds one protocol slot and delegates to the rest of the chain when the
//! slot declines the request. The chain terminates in a [`Fallback`] wrapping a
//! user-supplied fallback service (defaulting to [`DefaultNotFoundService`]).
//!
//! # Ordering
//!
//! Each protocol declares a [`ProtocolMeta::PRIORITY`] (lower is checked first).
//! The chain is walked head-to-tail, so composition order must be ascending by
//! priority. A compile-time guard ([`ProtocolChain`]'s `_ORDER_OK`) rejects a
//! misordered chain. The concrete public protocols are spaced by 1000 to leave
//! room for additional (e.g. internal) protocols to slot in between:
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
//! [`ProtocolChain`] is open: any type implementing [`ProtocolSlot`] can be a
//! link, including protocols defined in downstream crates. Such a protocol
//! declares its own `PRIORITY` and is spliced into the chain at the appropriate
//! position; nothing in this crate needs to know about it.

use std::{
    convert::Infallible,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use http::{Request, Response};
use tower::Service;

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
/// [`ProtocolChain`] before dispatching.
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

/// Compile-time metadata for a protocol slot, independent of the request/response
/// types. Split out from [`ProtocolSlot`] so the ordering guard and protocol-id
/// collection can be expressed without the `B`/`RespBody`/`E` type parameters.
pub trait ProtocolMeta {
    /// Detection priority; lower is checked earlier in the chain. Public
    /// protocols use multiples of 1000 (see the module table) so downstream
    /// protocols can slot in between.
    const PRIORITY: u16;

    /// The absolute Smithy shape ID of this protocol (e.g.
    /// `"aws.protocols#restJson1"`). Inserted into request extensions as
    /// [`SelectedProtocol`] when this slot handles a request.
    fn protocol_id(&self) -> &'static str;
}

/// An unused slot carries no protocol and parks at the end of the priority order.
impl ProtocolMeta for () {
    const PRIORITY: u16 = u16::MAX;

    #[inline(always)]
    fn protocol_id(&self) -> &'static str {
        "" // never used: `()`'s `can_handle` always returns `None`
    }
}

// ============================================================================
// ProtocolSlot (zero-cost protocol detection)
// ============================================================================

/// A protocol slot: detects whether it can handle a request, and if so, handles it.
///
/// The `Match` associated type carries any work done during detection through to
/// [`call`](ProtocolSlot::call), avoiding recomputation. For cheap header-only
/// detection it is `()`; for protocols that must match a route it caches the
/// matched service. The unused slot `()` uses `Infallible`, so its detection
/// branch is provably dead and eliminated by the compiler.
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

/// Unused protocol slot. `Option<Infallible>` can only be `None`, so the chain's
/// `if let Some(..)` branch for this slot is dead code the compiler removes.
impl<B, RespBody, E> ProtocolSlot<B, RespBody, E> for () {
    type Future = std::future::Pending<Result<Response<RespBody>, E>>;
    type Match = Infallible;

    #[inline(always)]
    fn can_handle(&self, _req: &Request<B>) -> Option<Self::Match> {
        None
    }

    fn call(&mut self, _req: Request<B>, matched: Self::Match) -> Self::Future {
        match matched {} // Infallible can never be constructed
    }
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
// ProtocolStack (chain terminal-or-link, carries HEAD_PRIORITY)
// ============================================================================

/// Implemented by every valid chain shape ([`Fallback`] terminal and
/// [`ProtocolChain`] links). Exposes the priority of the chain's first slot so
/// the next link out can assert the chain stays in ascending priority order.
pub trait ProtocolStack {
    /// Priority of the first protocol slot in this (sub)chain; `u16::MAX` for a
    /// bare [`Fallback`] terminal (no slots).
    const HEAD_PRIORITY: u16;
}

/// Collects the shape IDs of every protocol in a chain, in chain (priority) order.
pub trait ProtocolIds {
    /// Returns the ordered list of protocol shape IDs this chain supports.
    fn protocol_ids(&self) -> Vec<&'static str>;
}

// ============================================================================
// Fallback (chain terminal)
// ============================================================================

/// Terminal of a [`ProtocolChain`]: the service invoked when no protocol matches.
///
/// Wrapping the user's fallback gives the chain a typed end that participates in
/// the priority-order guard (as `HEAD_PRIORITY = u16::MAX`).
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

impl<F> ProtocolIds for Fallback<F> {
    fn protocol_ids(&self) -> Vec<&'static str> {
        Vec::new()
    }
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
// ProtocolChain (one link: try `head`, else delegate to `tail`)
// ============================================================================

/// One link of a multi-protocol chain: try `head`, and if it declines, delegate
/// to `tail` (the rest of the chain, ending in a [`Fallback`]).
///
/// Composition order is priority order: `head` is checked before anything in
/// `tail`. A compile-time guard rejects chains where a higher-priority slot is
/// placed behind a lower-priority one.
#[derive(Clone, Debug)]
pub struct ProtocolChain<Head, Tail> {
    head: Head,
    tail: Tail,
}

impl<Head, Tail> ProtocolChain<Head, Tail> {
    /// Creates a chain link. Prefer [`Push::push`] for building chains.
    pub fn new(head: Head, tail: Tail) -> Self {
        ProtocolChain { head, tail }
    }
}

// Compile-time order guard. Evaluating this const fails the build if a
// higher-priority slot (`head`) sits ahead of a lower-priority `tail`. `()`
// uses `PRIORITY = u16::MAX`, so unused slots never trip it.
impl<Head: ProtocolMeta, Tail: ProtocolStack> ProtocolChain<Head, Tail> {
    const _ORDER_OK: () = assert!(
        Head::PRIORITY <= Tail::HEAD_PRIORITY,
        "protocol chain is out of priority order: a higher-priority protocol is placed after a lower-priority one",
    );
}

impl<Head: ProtocolMeta, Tail: ProtocolStack> ProtocolStack for ProtocolChain<Head, Tail> {
    const HEAD_PRIORITY: u16 = Head::PRIORITY;
}

impl<Head: ProtocolMeta, Tail: ProtocolIds> ProtocolIds for ProtocolChain<Head, Tail> {
    fn protocol_ids(&self) -> Vec<&'static str> {
        let mut ids = Vec::new();
        ids.push(self.head.protocol_id());
        ids.extend(self.tail.protocol_ids());
        ids
    }
}

impl<B, Head, Tail, RespBody, E> Service<Request<B>> for ProtocolChain<Head, Tail>
where
    Head: ProtocolSlot<B, RespBody, E>,
    Tail: ProtocolStack + Service<Request<B>, Response = Response<RespBody>, Error = E>,
    Tail::Future: Future<Output = Result<Response<RespBody>, E>>,
{
    type Response = Response<RespBody>;
    type Error = E;
    type Future = ProtocolChainFuture<Head::Future, Tail::Future>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut req: Request<B>) -> Self::Future {
        // Force evaluation of the compile-time order guard.
        let () = Self::_ORDER_OK;

        // For a `()` head the compiler sees `if let Some(_) = None::<Infallible>`
        // and removes this branch entirely.
        if let Some(matched) = self.head.can_handle(&req) {
            let id = self.head.protocol_id();
            tracing::debug!(protocol = %id, "multi-protocol routing: request matched protocol");
            req.extensions_mut().insert(SelectedProtocol(id));
            ProtocolChainFuture::Head {
                fut: self.head.call(req, matched),
            }
        } else {
            ProtocolChainFuture::Tail {
                fut: self.tail.call(req),
            }
        }
    }
}

pin_project_lite::pin_project! {
    /// Response future for [`ProtocolChain`]: either the matched head's future or
    /// the tail's.
    #[project = ProtocolChainFutureProj]
    pub enum ProtocolChainFuture<HeadFut, TailFut> {
        Head { #[pin] fut: HeadFut },
        Tail { #[pin] fut: TailFut },
    }
}

impl<HeadFut, TailFut, RespBody, E> Future for ProtocolChainFuture<HeadFut, TailFut>
where
    HeadFut: Future<Output = Result<Response<RespBody>, E>>,
    TailFut: Future<Output = Result<Response<RespBody>, E>>,
{
    type Output = Result<Response<RespBody>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            ProtocolChainFutureProj::Head { fut } => fut.poll(cx),
            ProtocolChainFutureProj::Tail { fut } => fut.poll(cx),
        }
    }
}

// ============================================================================
// Builder (Push)
// ============================================================================

/// Prepends a protocol slot to a chain, making it the new head (checked first).
///
/// Because `push` prepends, build a chain by pushing the **lowest**-priority
/// protocol first and the **highest**-priority (lowest number) last, so the
/// resulting chain is in ascending priority order:
///
/// ```ignore
/// // RpcV2Cbor(1000) -> RestJson1(4000) -> Fallback
/// let chain = Fallback::not_found()
///     .push(rest_json1_routing_service) // 4000, pushed first
///     .push(cbor_routing_service);      // 1000, pushed last -> head
/// ```
pub trait Push: Sized {
    /// Prepends `slot`, returning a new chain with `slot` as head and `self` as tail.
    fn push<S>(self, slot: S) -> ProtocolChain<S, Self> {
        ProtocolChain { head: slot, tail: self }
    }
}

impl<F> Push for Fallback<F> {}
impl<Head, Tail> Push for ProtocolChain<Head, Tail> {}

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

impl_header_detected_slot!(CborRoutingService, crate::protocol::rpc_v2_cbor::RpcV2Cbor, 1000, is_rpc_v2_cbor);
impl_header_detected_slot!(AwsJson11RoutingService, crate::protocol::aws_json_11::AwsJson1_1, 2000, |req| {
    has_aws_json_target(req) && is_aws_json_11_content_type(req)
});
impl_header_detected_slot!(AwsJson10RoutingService, crate::protocol::aws_json_10::AwsJson1_0, 3000, |req| {
    has_aws_json_target(req) && is_aws_json_10_content_type(req)
});

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
    fn test_protocol_slot_for_unit_returns_none() {
        let mut unit: () = ();
        let req = Request::builder()
            .method(http::Method::GET)
            .uri("/foo")
            .body(())
            .unwrap();

        let result: Option<Infallible> = ProtocolSlot::<(), BoxBody, Infallible>::can_handle(&unit, &req);
        assert!(result.is_none());
        let _ = &mut unit;
    }

    #[test]
    fn test_unit_meta_parks_at_end() {
        assert_eq!(<() as ProtocolMeta>::PRIORITY, u16::MAX);
        assert_eq!(().protocol_id(), "");
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
        assert_eq!(<Fallback<DefaultNotFoundService> as ProtocolStack>::HEAD_PRIORITY, u16::MAX);
    }

    #[test]
    fn test_fallback_protocol_ids_empty() {
        assert!(Fallback::not_found().protocol_ids().is_empty());
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
    fn test_chain_builds_and_reports_ids_in_priority_order() {
        use crate::routing::request_spec::RequestSpec;

        // Build using the same reverse-push order codegen will use.
        let rest_router: RestRouter<()> = Vec::<(RequestSpec, ())>::new().into_iter().collect();
        let cbor_router: RpcV2CborRouter<()> = Vec::<(&'static str, ())>::new().into_iter().collect();

        let chain = Fallback::not_found()
            .push(RestJson1RoutingService::new(rest_router))
            .push(CborRoutingService::new(cbor_router));

        assert_eq!(
            chain.protocol_ids(),
            vec!["smithy.protocols#rpcv2Cbor", "aws.protocols#restJson1"]
        );

        // The chain's head priority is RpcV2Cbor's (1000), read off the concrete type.
        fn head_priority<T: ProtocolStack>(_: &T) -> u16 {
            T::HEAD_PRIORITY
        }
        assert_eq!(head_priority(&chain), 1000);
    }
}
