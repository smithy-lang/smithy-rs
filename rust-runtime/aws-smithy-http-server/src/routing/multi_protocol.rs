/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Multi-protocol routing support for Smithy services.
//!
//! This module provides [`MultiProtocolRoutingService`] which allows a single service
//! to support multiple Smithy protocols (e.g., RestJson1, RpcV2Cbor) simultaneously.
//!
//! Protocol detection order (most specific first):
//! 1. RpcV2Cbor - detected via `smithy-protocol: rpc-v2-cbor` header
//! 2. AwsJson1.1 - detected via `x-amz-target` header + Content-Type
//! 3. AwsJson1.0 - detected via `x-amz-target` header + Content-Type
//! 4. RestJson1 - detected via Content-Type + route matching
//! 5. RestXml - detected via Content-Type + route matching

use std::{
    convert::Infallible,
    fmt,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use http::{Request, Response};
use tower::Service;

use crate::{
    body::BoxBody,
    protocol::{aws_json::router::AwsJsonRouter, rest::router::RestRouter, rpc_v2_cbor::router::RpcV2CborRouter},
    routing::{Router, RoutingService},
};

// ============================================================================
// Protocol Enum (for request extensions)
// ============================================================================

/// The protocol that was used to handle a request.
///
/// This enum is inserted into request extensions by [`MultiProtocolRoutingService`]
/// before routing the request. Users can access it to determine which protocol
/// handled their request:
///
/// ```ignore
/// fn handler(req: http::Request<Body>) {
///     if let Some(protocol) = req.extensions().get::<Protocol>() {
///         match protocol {
///             Protocol::RestJson1 => println!("Handled by RestJson1"),
///             Protocol::RpcV2Cbor => println!("Handled by RpcV2Cbor"),
///             // ...
///         }
///     }
/// }
/// ```
/// Protocols are listed in detection priority order (most specific first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Protocol {
    /// Smithy RPC v2 CBOR protocol (checked 1st - most specific, has dedicated header)
    RpcV2Cbor,
    /// AWS JSON 1.1 protocol (checked 2nd - has x-amz-target + specific content-type)
    AwsJson1_1,
    /// AWS JSON 1.0 protocol (checked 3rd - has x-amz-target + specific content-type)
    AwsJson1_0,
    /// AWS RestJson1 protocol (checked 4th - content-type + route matching)
    RestJson1,
    /// AWS RestXml protocol (checked 5th - content-type + route matching)
    RestXml,
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Protocol::RpcV2Cbor => write!(f, "RpcV2Cbor"),
            Protocol::AwsJson1_1 => write!(f, "AwsJson1_1"),
            Protocol::AwsJson1_0 => write!(f, "AwsJson1_0"),
            Protocol::RestJson1 => write!(f, "RestJson1"),
            Protocol::RestXml => write!(f, "RestXml"),
        }
    }
}

// ============================================================================
// Routing Service Type Aliases
// ============================================================================

type CborRoutingService<S> = RoutingService<RpcV2CborRouter<S>, crate::protocol::rpc_v2_cbor::RpcV2Cbor>;
type AwsJson11RoutingService<S> = RoutingService<AwsJsonRouter<S>, crate::protocol::aws_json_11::AwsJson1_1>;
type AwsJson10RoutingService<S> = RoutingService<AwsJsonRouter<S>, crate::protocol::aws_json_10::AwsJson1_0>;
type RestJson1RoutingService<S> = RoutingService<RestRouter<S>, crate::protocol::rest_json_1::RestJson1>;
type RestXmlRoutingService<S> = RoutingService<RestRouter<S>, crate::protocol::rest_xml::RestXml>;

// ============================================================================
// ProtocolSlot Trait (Zero-Cost Protocol Detection)
// ============================================================================

/// Trait for protocol slot handling with zero-cost abstraction.
///
/// This trait abstracts over both configured protocol slots (real routing services)
/// and unused slots (`()`). The key insight is that for `()`, the compiler can
/// eliminate branches entirely because `Option<Infallible>` is always `None`.
///
/// The `Match` associated type allows caching expensive operations (like route matching)
/// so they don't need to be repeated in `call()`.
pub trait ProtocolSlot<B, RespBody, E> {
    /// The future type returned by `call()`.
    type Future: Future<Output = Result<Response<RespBody>, E>>;

    /// The "proof" that we can handle this request.
    /// For cheap protocol detection (header checks), this is `()`.
    /// For expensive detection (route matching), this caches the matched service.
    type Match;

    /// Check if this protocol can handle the given request.
    /// Returns `Some(match_result)` if it can handle it, `None` otherwise.
    fn can_handle(&self, req: &Request<B>) -> Option<Self::Match>;

    /// Handle the request, using the cached match result from `can_handle()`.
    fn call(&mut self, req: Request<B>, matched: Self::Match) -> Self::Future;
}

/// Implementation for `()` - unused protocol slots.
///
/// The compiler sees `Option<Infallible>` which can only be `None`,
/// so the entire `if let Some(...)` branch gets eliminated as dead code.
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
// DefaultNotFoundService (Fallback)
// ============================================================================

/// Default fallback service that returns a 404 Not Found response.
///
/// This is used when no protocol matches the incoming request.
/// Users can replace this with their own fallback service.
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
// MultiProtocolService
// ============================================================================

/// A multi-protocol routing service with zero-cost abstraction for unused protocols.
///
/// Each protocol slot defaults to `()`, which the compiler eliminates entirely.
/// The `Fallback` service is called when no protocol matches.
#[derive(Clone, Debug)]
pub struct MultiProtocolService<
    RpcV2 = (),
    AwsJson11 = (),
    AwsJson10 = (),
    RestJson = (),
    RestXml = (),
    Fallback = DefaultNotFoundService,
> {
    rpc_v2: RpcV2,
    aws_json_11: AwsJson11,
    aws_json_10: AwsJson10,
    rest_json: RestJson,
    rest_xml: RestXml,
    fallback: Fallback,
}

impl Default for MultiProtocolService<(), (), (), (), (), DefaultNotFoundService> {
    fn default() -> Self {
        Self::new()
    }
}

impl MultiProtocolService<(), (), (), (), (), DefaultNotFoundService> {
    /// Create a new multi-protocol service with no protocols configured.
    pub fn new() -> Self {
        Self {
            rpc_v2: (),
            aws_json_11: (),
            aws_json_10: (),
            rest_json: (),
            rest_xml: (),
            fallback: DefaultNotFoundService,
        }
    }
}

impl<RpcV2, AwsJson11, AwsJson10, RestJson, RestXml, Fallback>
    MultiProtocolService<RpcV2, AwsJson11, AwsJson10, RestJson, RestXml, Fallback>
{
    /// Add an RpcV2Cbor protocol router.
    pub fn with_rpc_v2_cbor<S>(
        self,
        svc: CborRoutingService<S>,
    ) -> MultiProtocolService<CborRoutingService<S>, AwsJson11, AwsJson10, RestJson, RestXml, Fallback> {
        MultiProtocolService {
            rpc_v2: svc,
            aws_json_11: self.aws_json_11,
            aws_json_10: self.aws_json_10,
            rest_json: self.rest_json,
            rest_xml: self.rest_xml,
            fallback: self.fallback,
        }
    }

    /// Add an AwsJson1.1 protocol router.
    pub fn with_aws_json_11<S>(
        self,
        svc: AwsJson11RoutingService<S>,
    ) -> MultiProtocolService<RpcV2, AwsJson11RoutingService<S>, AwsJson10, RestJson, RestXml, Fallback> {
        MultiProtocolService {
            rpc_v2: self.rpc_v2,
            aws_json_11: svc,
            aws_json_10: self.aws_json_10,
            rest_json: self.rest_json,
            rest_xml: self.rest_xml,
            fallback: self.fallback,
        }
    }

    /// Add an AwsJson1.0 protocol router.
    pub fn with_aws_json_10<S>(
        self,
        svc: AwsJson10RoutingService<S>,
    ) -> MultiProtocolService<RpcV2, AwsJson11, AwsJson10RoutingService<S>, RestJson, RestXml, Fallback> {
        MultiProtocolService {
            rpc_v2: self.rpc_v2,
            aws_json_11: self.aws_json_11,
            aws_json_10: svc,
            rest_json: self.rest_json,
            rest_xml: self.rest_xml,
            fallback: self.fallback,
        }
    }

    /// Add a RestJson1 protocol router.
    pub fn with_rest_json1<S>(
        self,
        svc: RestJson1RoutingService<S>,
    ) -> MultiProtocolService<RpcV2, AwsJson11, AwsJson10, RestJson1RoutingService<S>, RestXml, Fallback> {
        MultiProtocolService {
            rpc_v2: self.rpc_v2,
            aws_json_11: self.aws_json_11,
            aws_json_10: self.aws_json_10,
            rest_json: svc,
            rest_xml: self.rest_xml,
            fallback: self.fallback,
        }
    }

    /// Add a RestXml protocol router.
    pub fn with_rest_xml<S>(
        self,
        svc: RestXmlRoutingService<S>,
    ) -> MultiProtocolService<RpcV2, AwsJson11, AwsJson10, RestJson, RestXmlRoutingService<S>, Fallback> {
        MultiProtocolService {
            rpc_v2: self.rpc_v2,
            aws_json_11: self.aws_json_11,
            aws_json_10: self.aws_json_10,
            rest_json: self.rest_json,
            rest_xml: svc,
            fallback: self.fallback,
        }
    }

    /// Set a custom fallback service for when no protocol matches.
    pub fn fallback<F>(self, fallback: F) -> MultiProtocolService<RpcV2, AwsJson11, AwsJson10, RestJson, RestXml, F> {
        MultiProtocolService {
            rpc_v2: self.rpc_v2,
            aws_json_11: self.aws_json_11,
            aws_json_10: self.aws_json_10,
            rest_json: self.rest_json,
            rest_xml: self.rest_xml,
            fallback,
        }
    }
}

// ============================================================================
// ProtocolSlot Implementations for Routing Services
// ============================================================================

/// ProtocolSlot for RpcV2Cbor - uses header check only (cheap).
impl<S, B, RespBody, E> ProtocolSlot<B, RespBody, E> for CborRoutingService<S>
where
    CborRoutingService<S>: Service<Request<B>, Response = Response<RespBody>, Error = E>,
    <CborRoutingService<S> as Service<Request<B>>>::Future: Future<Output = Result<Response<RespBody>, E>>,
{
    type Future = <CborRoutingService<S> as Service<Request<B>>>::Future;
    type Match = ();

    #[inline]
    fn can_handle(&self, req: &Request<B>) -> Option<Self::Match> {
        is_rpc_v2_cbor(req).then_some(())
    }

    fn call(&mut self, req: Request<B>, _matched: Self::Match) -> Self::Future {
        Service::call(self, req)
    }
}

/// ProtocolSlot for AwsJson1.1 - uses header checks only (cheap).
impl<S, B, RespBody, E> ProtocolSlot<B, RespBody, E> for AwsJson11RoutingService<S>
where
    AwsJson11RoutingService<S>: Service<Request<B>, Response = Response<RespBody>, Error = E>,
    <AwsJson11RoutingService<S> as Service<Request<B>>>::Future: Future<Output = Result<Response<RespBody>, E>>,
{
    type Future = <AwsJson11RoutingService<S> as Service<Request<B>>>::Future;
    type Match = ();

    #[inline]
    fn can_handle(&self, req: &Request<B>) -> Option<Self::Match> {
        (has_aws_json_target(req) && is_aws_json_11_content_type(req)).then_some(())
    }

    fn call(&mut self, req: Request<B>, _matched: Self::Match) -> Self::Future {
        Service::call(self, req)
    }
}

/// ProtocolSlot for AwsJson1.0 - uses header checks only (cheap).
impl<S, B, RespBody, E> ProtocolSlot<B, RespBody, E> for AwsJson10RoutingService<S>
where
    AwsJson10RoutingService<S>: Service<Request<B>, Response = Response<RespBody>, Error = E>,
    <AwsJson10RoutingService<S> as Service<Request<B>>>::Future: Future<Output = Result<Response<RespBody>, E>>,
{
    type Future = <AwsJson10RoutingService<S> as Service<Request<B>>>::Future;
    type Match = ();

    #[inline]
    fn can_handle(&self, req: &Request<B>) -> Option<Self::Match> {
        (has_aws_json_target(req) && is_aws_json_10_content_type(req)).then_some(())
    }

    fn call(&mut self, req: Request<B>, _matched: Self::Match) -> Self::Future {
        Service::call(self, req)
    }
}

/// ProtocolSlot for RestJson1 - uses content-type + route matching (expensive).
/// The Match type caches the matched route to avoid double matching.
impl<S, B, RespBody, E> ProtocolSlot<B, RespBody, E> for RestJson1RoutingService<S>
where
    RestRouter<S>: Router<B, Service = S>,
    S: Clone + Service<Request<B>, Response = Response<RespBody>, Error = E>,
    <S as Service<Request<B>>>::Future: Future<Output = Result<Response<RespBody>, E>>,
{
    type Future = <S as Service<Request<B>>>::Future;
    type Match = S; // Cache the matched service

    #[inline]
    fn can_handle(&self, req: &Request<B>) -> Option<Self::Match> {
        if is_json_content_type(req) {
            self.router().match_route(req).ok()
        } else {
            None
        }
    }

    fn call(&mut self, req: Request<B>, mut matched: Self::Match) -> Self::Future {
        matched.call(req)
    }
}

/// ProtocolSlot for RestXml - uses content-type + route matching (expensive).
/// The Match type caches the matched route to avoid double matching.
impl<S, B, RespBody, E> ProtocolSlot<B, RespBody, E> for RestXmlRoutingService<S>
where
    RestRouter<S>: Router<B, Service = S>,
    S: Clone + Service<Request<B>, Response = Response<RespBody>, Error = E>,
    <S as Service<Request<B>>>::Future: Future<Output = Result<Response<RespBody>, E>>,
{
    type Future = <S as Service<Request<B>>>::Future;
    type Match = S; // Cache the matched service

    #[inline]
    fn can_handle(&self, req: &Request<B>) -> Option<Self::Match> {
        if is_xml_content_type(req) {
            self.router().match_route(req).ok()
        } else {
            None
        }
    }

    fn call(&mut self, req: Request<B>, mut matched: Self::Match) -> Self::Future {
        matched.call(req)
    }
}

// ============================================================================
// MultiProtocolFuture
// ============================================================================

pin_project_lite::pin_project! {
    /// The response future for [`MultiProtocolService`].
    ///
    /// This enum unifies the different future types from each protocol.
    /// For unused protocol slots (`()`), the future type is `Pending` which is zero-sized
    /// and never constructed.
    #[project = MultiProtocolFutureProj]
    pub enum MultiProtocolFuture<RpcV2Fut, AwsJson11Fut, AwsJson10Fut, RestJsonFut, RestXmlFut, FallbackFut> {
        RpcV2 { #[pin] fut: RpcV2Fut },
        AwsJson11 { #[pin] fut: AwsJson11Fut },
        AwsJson10 { #[pin] fut: AwsJson10Fut },
        RestJson { #[pin] fut: RestJsonFut },
        RestXml { #[pin] fut: RestXmlFut },
        Fallback { #[pin] fut: FallbackFut },
    }
}

impl<RpcV2Fut, AwsJson11Fut, AwsJson10Fut, RestJsonFut, RestXmlFut, FallbackFut, RespBody, E> Future
    for MultiProtocolFuture<RpcV2Fut, AwsJson11Fut, AwsJson10Fut, RestJsonFut, RestXmlFut, FallbackFut>
where
    RpcV2Fut: Future<Output = Result<Response<RespBody>, E>>,
    AwsJson11Fut: Future<Output = Result<Response<RespBody>, E>>,
    AwsJson10Fut: Future<Output = Result<Response<RespBody>, E>>,
    RestJsonFut: Future<Output = Result<Response<RespBody>, E>>,
    RestXmlFut: Future<Output = Result<Response<RespBody>, E>>,
    FallbackFut: Future<Output = Result<Response<RespBody>, E>>,
{
    type Output = Result<Response<RespBody>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            MultiProtocolFutureProj::RpcV2 { fut } => fut.poll(cx),
            MultiProtocolFutureProj::AwsJson11 { fut } => fut.poll(cx),
            MultiProtocolFutureProj::AwsJson10 { fut } => fut.poll(cx),
            MultiProtocolFutureProj::RestJson { fut } => fut.poll(cx),
            MultiProtocolFutureProj::RestXml { fut } => fut.poll(cx),
            MultiProtocolFutureProj::Fallback { fut } => fut.poll(cx),
        }
    }
}

// ============================================================================
// Service Implementation for MultiProtocolService
// ============================================================================

impl<B, RpcV2, AwsJson11, AwsJson10, RestJson, RestXml, Fallback, RespBody, E> Service<Request<B>>
    for MultiProtocolService<RpcV2, AwsJson11, AwsJson10, RestJson, RestXml, Fallback>
where
    RpcV2: ProtocolSlot<B, RespBody, E>,
    AwsJson11: ProtocolSlot<B, RespBody, E>,
    AwsJson10: ProtocolSlot<B, RespBody, E>,
    RestJson: ProtocolSlot<B, RespBody, E>,
    RestXml: ProtocolSlot<B, RespBody, E>,
    Fallback: Service<Request<B>, Response = Response<RespBody>, Error = E>,
    Fallback::Future: Future<Output = Result<Response<RespBody>, E>>,
{
    type Response = Response<RespBody>;
    type Error = E;
    type Future = MultiProtocolFuture<
        RpcV2::Future,
        AwsJson11::Future,
        AwsJson10::Future,
        RestJson::Future,
        RestXml::Future,
        Fallback::Future,
    >;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        // Try protocols in order of specificity.
        // For () slots, compiler eliminates the branch entirely since
        // Option<Infallible> can only be None.

        if let Some(matched) = self.rpc_v2.can_handle(&req) {
            return MultiProtocolFuture::RpcV2 {
                fut: self.rpc_v2.call(req, matched),
            };
        }
        if let Some(matched) = self.aws_json_11.can_handle(&req) {
            return MultiProtocolFuture::AwsJson11 {
                fut: self.aws_json_11.call(req, matched),
            };
        }
        if let Some(matched) = self.aws_json_10.can_handle(&req) {
            return MultiProtocolFuture::AwsJson10 {
                fut: self.aws_json_10.call(req, matched),
            };
        }
        if let Some(matched) = self.rest_json.can_handle(&req) {
            return MultiProtocolFuture::RestJson {
                fut: self.rest_json.call(req, matched),
            };
        }
        if let Some(matched) = self.rest_xml.can_handle(&req) {
            return MultiProtocolFuture::RestXml {
                fut: self.rest_xml.call(req, matched),
            };
        }

        // No protocol matched - use fallback
        MultiProtocolFuture::Fallback {
            fut: self.fallback.call(req),
        }
    }
}

// ============================================================================
// Protocol Detection Functions
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
fn is_json_content_type<B>(req: &Request<B>) -> bool {
    req.headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("application/json") || v.contains("+json"))
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

        // Without header
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

        // No content-type defaults to true (for GET requests etc.)
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
    fn test_multi_protocol_service_builder() {
        // Test that we can create an empty service
        let _service = MultiProtocolService::new();

        // Test that Default works
        let _service: MultiProtocolService = Default::default();
    }

    #[test]
    fn test_protocol_slot_for_unit_returns_none() {
        // () should always return None from can_handle
        let unit: () = ();
        let req = Request::builder()
            .method(http::Method::GET)
            .uri("/foo")
            .body(())
            .unwrap();

        let result: Option<Infallible> = ProtocolSlot::<(), BoxBody, Infallible>::can_handle(&unit, &req);
        assert!(result.is_none());
    }

    #[test]
    fn test_default_not_found_service() {
        use std::task::Poll;

        let mut service = DefaultNotFoundService;

        // poll_ready should return Ready(Ok(()))
        let waker = futures_util::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        assert!(matches!(
            Service::<Request<()>>::poll_ready(&mut service, &mut cx),
            Poll::Ready(Ok(()))
        ));
    }

    #[test]
    fn test_protocol_display() {
        assert_eq!(format!("{}", Protocol::RpcV2Cbor), "RpcV2Cbor");
        assert_eq!(format!("{}", Protocol::AwsJson1_1), "AwsJson1_1");
        assert_eq!(format!("{}", Protocol::AwsJson1_0), "AwsJson1_0");
        assert_eq!(format!("{}", Protocol::RestJson1), "RestJson1");
        assert_eq!(format!("{}", Protocol::RestXml), "RestXml");
    }

    #[test]
    fn test_multi_protocol_service_is_clone() {
        let service = MultiProtocolService::new();
        let _cloned = service.clone();
    }

    #[test]
    fn test_multi_protocol_service_is_debug() {
        let service = MultiProtocolService::new();
        let debug_str = format!("{:?}", service);
        assert!(debug_str.contains("MultiProtocolService"));
    }

    #[test]
    fn test_custom_fallback_service() {
        use std::convert::Infallible;

        // Define a custom fallback service that returns 418 I'm a teapot
        #[derive(Clone, Debug)]
        struct TeapotService;

        impl<B> Service<Request<B>> for TeapotService {
            type Response = Response<BoxBody>;
            type Error = Infallible;
            type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

            fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
                Poll::Ready(Ok(()))
            }

            fn call(&mut self, _req: Request<B>) -> Self::Future {
                std::future::ready(Ok(Response::builder()
                    .status(http::StatusCode::IM_A_TEAPOT)
                    .body(crate::body::to_boxed("I'm a teapot"))
                    .unwrap()))
            }
        }

        // Test that we can set a custom fallback
        let service = MultiProtocolService::new().fallback(TeapotService);

        // Verify the type changed (this compiles = type system works)
        let _: MultiProtocolService<(), (), (), (), (), TeapotService> = service;
    }
}
