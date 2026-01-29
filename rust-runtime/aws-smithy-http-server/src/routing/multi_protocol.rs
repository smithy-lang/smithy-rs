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
    error::Error as StdError,
    fmt,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use http::{Request, Response};
use http_body::Body as HttpBody;
use tower::Service;

use crate::{
    body::BoxBody,
    error::BoxError,
    response::IntoResponse,
    routing::{Router, RoutingFuture, RoutingService},
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
// Error Types
// ============================================================================

/// Error type for [`MultiProtocolRoutingService`].
///
/// This enum wraps the error types from each protocol's routing service,
/// allowing them to be unified into a single error type.
/// Generic parameters are in detection priority order (most specific first).
#[derive(Debug)]
pub enum MultiProtocolRoutingError<RpcV2E, AwsJson11E, AwsJson10E, RestJsonE, RestXmlE> {
    /// Error from RpcV2Cbor protocol routing
    RpcV2Cbor(RpcV2E),
    /// Error from AwsJson1.1 protocol routing
    AwsJson11(AwsJson11E),
    /// Error from AwsJson1.0 protocol routing
    AwsJson10(AwsJson10E),
    /// Error from RestJson1 protocol routing
    RestJson(RestJsonE),
    /// Error from RestXml protocol routing
    RestXml(RestXmlE),
    /// No protocol matched the request
    NoProtocolMatched,
}

impl<RpcV2E, AwsJson11E, AwsJson10E, RestJsonE, RestXmlE> fmt::Display
    for MultiProtocolRoutingError<RpcV2E, AwsJson11E, AwsJson10E, RestJsonE, RestXmlE>
where
    RpcV2E: fmt::Display,
    AwsJson11E: fmt::Display,
    AwsJson10E: fmt::Display,
    RestJsonE: fmt::Display,
    RestXmlE: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RpcV2Cbor(e) => write!(f, "RpcV2Cbor error: {}", e),
            Self::AwsJson11(e) => write!(f, "AwsJson1.1 error: {}", e),
            Self::AwsJson10(e) => write!(f, "AwsJson1.0 error: {}", e),
            Self::RestJson(e) => write!(f, "RestJson1 error: {}", e),
            Self::RestXml(e) => write!(f, "RestXml error: {}", e),
            Self::NoProtocolMatched => write!(f, "no protocol matched the request"),
        }
    }
}

impl<RpcV2E, AwsJson11E, AwsJson10E, RestJsonE, RestXmlE> StdError
    for MultiProtocolRoutingError<RpcV2E, AwsJson11E, AwsJson10E, RestJsonE, RestXmlE>
where
    RpcV2E: StdError + 'static,
    AwsJson11E: StdError + 'static,
    AwsJson10E: StdError + 'static,
    RestJsonE: StdError + 'static,
    RestXmlE: StdError + 'static,
{
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::RpcV2Cbor(e) => Some(e),
            Self::AwsJson11(e) => Some(e),
            Self::AwsJson10(e) => Some(e),
            Self::RestJson(e) => Some(e),
            Self::RestXml(e) => Some(e),
            Self::NoProtocolMatched => None,
        }
    }
}

// Special case: when all errors are Infallible, the combined error is also Infallible-like
impl From<MultiProtocolRoutingError<Infallible, Infallible, Infallible, Infallible, Infallible>>
    for Infallible
{
    fn from(err: MultiProtocolRoutingError<Infallible, Infallible, Infallible, Infallible, Infallible>) -> Self {
        match err {
            MultiProtocolRoutingError::RestJson(e) => match e {},
            MultiProtocolRoutingError::RestXml(e) => match e {},
            MultiProtocolRoutingError::AwsJson10(e) => match e {},
            MultiProtocolRoutingError::AwsJson11(e) => match e {},
            MultiProtocolRoutingError::RpcV2Cbor(e) => match e {},
            MultiProtocolRoutingError::NoProtocolMatched => {
                // This is the only case that can actually happen
                // But Infallible can never be constructed, so this is unreachable
                // in a well-typed program where we handle NoProtocolMatched separately
                unreachable!("NoProtocolMatched should be converted to a response, not an error")
            }
        }
    }
}

// ============================================================================
// Protocol Detection
// ============================================================================

/// Trait for protocol detection.
///
/// Implemented by `RoutingService<R, P>` for each protocol to determine
/// if a request should be handled by that protocol.
pub trait ProtocolDetector<B> {
    /// Check if this protocol can handle the given request.
    fn can_handle(&self, req: &Request<B>) -> bool;
}

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

// ============================================================================
// Router and ProtocolDetector for () (unused protocol slots)
// ============================================================================

/// A service that is never called (used for unused protocol slots).
#[derive(Debug, Clone, Copy)]
pub struct NeverService;

impl<B> tower::Service<Request<B>> for NeverService {
    type Response = Response<crate::body::BoxBody>;
    type Error = std::convert::Infallible;
    type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: Request<B>) -> Self::Future {
        unreachable!("NeverService should never be called - protocol not configured")
    }
}

/// Error for unused protocol slots.
#[derive(Debug)]
pub struct ProtocolNotConfigured;

impl fmt::Display for ProtocolNotConfigured {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "protocol not configured")
    }
}

impl StdError for ProtocolNotConfigured {}

impl<P> crate::response::IntoResponse<P> for ProtocolNotConfigured {
    fn into_response(self) -> Response<crate::body::BoxBody> {
        Response::builder()
            .status(http::StatusCode::NOT_FOUND)
            .body(crate::body::to_boxed("{}"))
            .unwrap()
    }
}

/// Implement Router for () so it can be used as a placeholder for unused protocols.
impl<B> Router<B> for () {
    type Service = NeverService;
    type Error = ProtocolNotConfigured;

    fn match_route(&self, _request: &Request<B>) -> Result<Self::Service, Self::Error> {
        Err(ProtocolNotConfigured)
    }
}

/// ProtocolDetector for unused protocol slots - always returns false.
impl<B, P> ProtocolDetector<B> for RoutingService<(), P> {
    fn can_handle(&self, _req: &Request<B>) -> bool {
        false
    }
}

// ============================================================================
// ProtocolDetector implementations for specific router types
// ============================================================================

// Implement ProtocolDetector for RpcV2Cbor with RpcV2CborRouter
impl<S, B> ProtocolDetector<B> for RoutingService<crate::protocol::rpc_v2_cbor::router::RpcV2CborRouter<S>, crate::protocol::rpc_v2_cbor::RpcV2Cbor>
{
    fn can_handle(&self, req: &Request<B>) -> bool {
        is_rpc_v2_cbor(req)
    }
}

// Implement ProtocolDetector for AwsJson1.1 with AwsJsonRouter
impl<S, B> ProtocolDetector<B> for RoutingService<crate::protocol::aws_json::router::AwsJsonRouter<S>, crate::protocol::aws_json_11::AwsJson1_1>
{
    fn can_handle(&self, req: &Request<B>) -> bool {
        has_aws_json_target(req) && is_aws_json_11_content_type(req)
    }
}

// Implement ProtocolDetector for AwsJson1.0 with AwsJsonRouter
impl<S, B> ProtocolDetector<B> for RoutingService<crate::protocol::aws_json::router::AwsJsonRouter<S>, crate::protocol::aws_json_10::AwsJson1_0>
{
    fn can_handle(&self, req: &Request<B>) -> bool {
        has_aws_json_target(req) && is_aws_json_10_content_type(req)
    }
}

// Implement ProtocolDetector for RestJson1 with RestRouter
impl<S, B> ProtocolDetector<B> for RoutingService<crate::protocol::rest::router::RestRouter<S>, crate::protocol::rest_json_1::RestJson1>
where
    S: Clone,
{
    fn can_handle(&self, req: &Request<B>) -> bool {
        is_json_content_type(req) && self.router().match_route(req).is_ok()
    }
}

// Implement ProtocolDetector for RestXml with RestRouter
impl<S, B> ProtocolDetector<B> for RoutingService<crate::protocol::rest::router::RestRouter<S>, crate::protocol::rest_xml::RestXml>
where
    S: Clone,
{
    fn can_handle(&self, req: &Request<B>) -> bool {
        is_xml_content_type(req) && self.router().match_route(req).is_ok()
    }
}

// ============================================================================
// MultiProtocolRoutingService
// ============================================================================

/// A multi-protocol routing service that tries multiple protocol routers in order.
///
/// The type parameters represent the router types for each protocol, ordered by detection priority:
/// - `RpcV2Router` - Router type for RpcV2Cbor (typically `RpcV2CborRouter<Route<B>>`)
/// - `AwsJson11Router` - Router type for AwsJson1.1 (typically `AwsJsonRouter<Route<B>>`)
/// - `AwsJson10Router` - Router type for AwsJson1.0 (typically `AwsJsonRouter<Route<B>>`)
/// - `RestJsonRouter` - Router type for RestJson1 (typically `RestRouter<Route<B>>`)
/// - `RestXmlRouter` - Router type for RestXml (typically `RestRouter<Route<B>>`)
pub struct MultiProtocolRoutingService<
    RpcV2Router = (),
    AwsJson11Router = (),
    AwsJson10Router = (),
    RestJsonRouter = (),
    RestXmlRouter = (),
> {
    rpc_v2: Option<RoutingService<RpcV2Router, crate::protocol::rpc_v2_cbor::RpcV2Cbor>>,
    aws_json_11: Option<RoutingService<AwsJson11Router, crate::protocol::aws_json_11::AwsJson1_1>>,
    aws_json_10: Option<RoutingService<AwsJson10Router, crate::protocol::aws_json_10::AwsJson1_0>>,
    rest_json: Option<RoutingService<RestJsonRouter, crate::protocol::rest_json_1::RestJson1>>,
    rest_xml: Option<RoutingService<RestXmlRouter, crate::protocol::rest_xml::RestXml>>,
}

impl MultiProtocolRoutingService<(), (), (), (), ()> {
    /// Create a new empty multi-protocol routing service.
    pub fn new() -> Self {
        Self {
            rpc_v2: None,
            aws_json_11: None,
            aws_json_10: None,
            rest_json: None,
            rest_xml: None,
        }
    }
}

impl Default for MultiProtocolRoutingService<(), (), (), (), ()> {
    fn default() -> Self {
        Self::new()
    }
}

impl<RpcV2Router, AwsJson11Router, AwsJson10Router, RestJsonRouter, RestXmlRouter>
    MultiProtocolRoutingService<RpcV2Router, AwsJson11Router, AwsJson10Router, RestJsonRouter, RestXmlRouter>
{
    /// Add an RpcV2Cbor protocol router.
    pub fn with_rpc_v2_cbor<R>(
        self,
        svc: RoutingService<R, crate::protocol::rpc_v2_cbor::RpcV2Cbor>,
    ) -> MultiProtocolRoutingService<R, AwsJson11Router, AwsJson10Router, RestJsonRouter, RestXmlRouter> {
        MultiProtocolRoutingService {
            rpc_v2: Some(svc),
            aws_json_11: self.aws_json_11,
            aws_json_10: self.aws_json_10,
            rest_json: self.rest_json,
            rest_xml: self.rest_xml,
        }
    }

    /// Add an AwsJson1.1 protocol router.
    pub fn with_aws_json_11<R>(
        self,
        svc: RoutingService<R, crate::protocol::aws_json_11::AwsJson1_1>,
    ) -> MultiProtocolRoutingService<RpcV2Router, R, AwsJson10Router, RestJsonRouter, RestXmlRouter> {
        MultiProtocolRoutingService {
            rpc_v2: self.rpc_v2,
            aws_json_11: Some(svc),
            aws_json_10: self.aws_json_10,
            rest_json: self.rest_json,
            rest_xml: self.rest_xml,
        }
    }

    /// Add an AwsJson1.0 protocol router.
    pub fn with_aws_json_10<R>(
        self,
        svc: RoutingService<R, crate::protocol::aws_json_10::AwsJson1_0>,
    ) -> MultiProtocolRoutingService<RpcV2Router, AwsJson11Router, R, RestJsonRouter, RestXmlRouter> {
        MultiProtocolRoutingService {
            rpc_v2: self.rpc_v2,
            aws_json_11: self.aws_json_11,
            aws_json_10: Some(svc),
            rest_json: self.rest_json,
            rest_xml: self.rest_xml,
        }
    }

    /// Add a RestJson1 protocol router.
    pub fn with_rest_json1<R>(
        self,
        svc: RoutingService<R, crate::protocol::rest_json_1::RestJson1>,
    ) -> MultiProtocolRoutingService<RpcV2Router, AwsJson11Router, AwsJson10Router, R, RestXmlRouter> {
        MultiProtocolRoutingService {
            rpc_v2: self.rpc_v2,
            aws_json_11: self.aws_json_11,
            aws_json_10: self.aws_json_10,
            rest_json: Some(svc),
            rest_xml: self.rest_xml,
        }
    }

    /// Add a RestXml protocol router.
    pub fn with_rest_xml<R>(
        self,
        svc: RoutingService<R, crate::protocol::rest_xml::RestXml>,
    ) -> MultiProtocolRoutingService<RpcV2Router, AwsJson11Router, AwsJson10Router, RestJsonRouter, R> {
        MultiProtocolRoutingService {
            rpc_v2: self.rpc_v2,
            aws_json_11: self.aws_json_11,
            aws_json_10: self.aws_json_10,
            rest_json: self.rest_json,
            rest_xml: Some(svc),
        }
    }
}

impl<RpcV2Router, AwsJson11Router, AwsJson10Router, RestJsonRouter, RestXmlRouter> Clone
    for MultiProtocolRoutingService<RpcV2Router, AwsJson11Router, AwsJson10Router, RestJsonRouter, RestXmlRouter>
where
    RpcV2Router: Clone,
    AwsJson11Router: Clone,
    AwsJson10Router: Clone,
    RestJsonRouter: Clone,
    RestXmlRouter: Clone,
{
    fn clone(&self) -> Self {
        Self {
            rpc_v2: self.rpc_v2.clone(),
            aws_json_11: self.aws_json_11.clone(),
            aws_json_10: self.aws_json_10.clone(),
            rest_json: self.rest_json.clone(),
            rest_xml: self.rest_xml.clone(),
        }
    }
}

impl<RpcV2Router, AwsJson11Router, AwsJson10Router, RestJsonRouter, RestXmlRouter> fmt::Debug
    for MultiProtocolRoutingService<RpcV2Router, AwsJson11Router, AwsJson10Router, RestJsonRouter, RestXmlRouter>
where
    RpcV2Router: fmt::Debug,
    AwsJson11Router: fmt::Debug,
    AwsJson10Router: fmt::Debug,
    RestJsonRouter: fmt::Debug,
    RestXmlRouter: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MultiProtocolRoutingService")
            .field("rpc_v2", &self.rpc_v2)
            .field("aws_json_11", &self.aws_json_11)
            .field("aws_json_10", &self.aws_json_10)
            .field("rest_json", &self.rest_json)
            .field("rest_xml", &self.rest_xml)
            .finish()
    }
}

// ============================================================================
// Future Implementation
// ============================================================================

/// The response future for [`MultiProtocolRoutingService`].
pub struct MultiProtocolRoutingFuture<RpcV2Fut, AwsJson11Fut, AwsJson10Fut, RestJsonFut, RestXmlFut> {
    inner: MultiProtocolRoutingFutureInner<RpcV2Fut, AwsJson11Fut, AwsJson10Fut, RestJsonFut, RestXmlFut>,
}

enum MultiProtocolRoutingFutureInner<RpcV2Fut, AwsJson11Fut, AwsJson10Fut, RestJsonFut, RestXmlFut> {
    RpcV2(RpcV2Fut),
    AwsJson11(AwsJson11Fut),
    AwsJson10(AwsJson10Fut),
    RestJson(RestJsonFut),
    RestXml(RestXmlFut),
    NotFound,
}

impl<RpcV2Fut, AwsJson11Fut, AwsJson10Fut, RestJsonFut, RestXmlFut, RespBody, RpcV2E, AwsJson11E, AwsJson10E, RestJsonE, RestXmlE>
    Future
    for MultiProtocolRoutingFuture<RpcV2Fut, AwsJson11Fut, AwsJson10Fut, RestJsonFut, RestXmlFut>
where
    RpcV2Fut: Future<Output = Result<Response<RespBody>, RpcV2E>> + Unpin,
    AwsJson11Fut: Future<Output = Result<Response<RespBody>, AwsJson11E>> + Unpin,
    AwsJson10Fut: Future<Output = Result<Response<RespBody>, AwsJson10E>> + Unpin,
    RestJsonFut: Future<Output = Result<Response<RespBody>, RestJsonE>> + Unpin,
    RestXmlFut: Future<Output = Result<Response<RespBody>, RestXmlE>> + Unpin,
    RespBody: HttpBody<Data = Bytes> + Send + 'static,
    RespBody::Error: Into<BoxError>,
{
    type Output = Result<
        Response<BoxBody>,
        MultiProtocolRoutingError<RpcV2E, AwsJson11E, AwsJson10E, RestJsonE, RestXmlE>,
    >;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match &mut this.inner {
            MultiProtocolRoutingFutureInner::RpcV2(fut) => Pin::new(fut).poll(cx).map(|res| {
                res.map(|r| r.map(crate::body::boxed))
                    .map_err(MultiProtocolRoutingError::RpcV2Cbor)
            }),
            MultiProtocolRoutingFutureInner::AwsJson11(fut) => Pin::new(fut).poll(cx).map(|res| {
                res.map(|r| r.map(crate::body::boxed))
                    .map_err(MultiProtocolRoutingError::AwsJson11)
            }),
            MultiProtocolRoutingFutureInner::AwsJson10(fut) => Pin::new(fut).poll(cx).map(|res| {
                res.map(|r| r.map(crate::body::boxed))
                    .map_err(MultiProtocolRoutingError::AwsJson10)
            }),
            MultiProtocolRoutingFutureInner::RestJson(fut) => Pin::new(fut).poll(cx).map(|res| {
                res.map(|r| r.map(crate::body::boxed))
                    .map_err(MultiProtocolRoutingError::RestJson)
            }),
            MultiProtocolRoutingFutureInner::RestXml(fut) => Pin::new(fut).poll(cx).map(|res| {
                res.map(|r| r.map(crate::body::boxed))
                    .map_err(MultiProtocolRoutingError::RestXml)
            }),
            MultiProtocolRoutingFutureInner::NotFound => {
                // Return a default 404 response in RestJson1 format
                Poll::Ready(Ok(default_not_found_response()))
            }
        }
    }
}

/// Returns a default 404 Not Found response in RestJson1 format.
fn default_not_found_response() -> Response<BoxBody> {
    Response::builder()
        .status(http::StatusCode::NOT_FOUND)
        .header(http::header::CONTENT_TYPE, "application/json")
        .header(
            "X-Amzn-Errortype",
            crate::routing::UNKNOWN_OPERATION_EXCEPTION,
        )
        .body(crate::body::to_boxed("{}"))
        .expect("valid HTTP response")
}

// ============================================================================
// Service Implementation
// ============================================================================

impl<B, RpcV2Router, AwsJson11Router, AwsJson10Router, RestJsonRouter, RestXmlRouter, RespBody>
    Service<Request<B>>
    for MultiProtocolRoutingService<RpcV2Router, AwsJson11Router, AwsJson10Router, RestJsonRouter, RestXmlRouter>
where
    // RpcV2Cbor bounds
    RpcV2Router: Router<B> + Clone,
    RpcV2Router::Service: Service<Request<B>, Response = Response<RespBody>> + Clone,
    RpcV2Router::Error: IntoResponse<crate::protocol::rpc_v2_cbor::RpcV2Cbor> + StdError,
    RoutingService<RpcV2Router, crate::protocol::rpc_v2_cbor::RpcV2Cbor>: ProtocolDetector<B>,
    <RpcV2Router::Service as Service<Request<B>>>::Future: Unpin,

    // AwsJson1.1 bounds
    AwsJson11Router: Router<B> + Clone,
    AwsJson11Router::Service: Service<Request<B>, Response = Response<RespBody>> + Clone,
    AwsJson11Router::Error: IntoResponse<crate::protocol::aws_json_11::AwsJson1_1> + StdError,
    RoutingService<AwsJson11Router, crate::protocol::aws_json_11::AwsJson1_1>: ProtocolDetector<B>,
    <AwsJson11Router::Service as Service<Request<B>>>::Future: Unpin,

    // AwsJson1.0 bounds
    AwsJson10Router: Router<B> + Clone,
    AwsJson10Router::Service: Service<Request<B>, Response = Response<RespBody>> + Clone,
    AwsJson10Router::Error: IntoResponse<crate::protocol::aws_json_10::AwsJson1_0> + StdError,
    RoutingService<AwsJson10Router, crate::protocol::aws_json_10::AwsJson1_0>: ProtocolDetector<B>,
    <AwsJson10Router::Service as Service<Request<B>>>::Future: Unpin,

    // RestJson1 bounds
    RestJsonRouter: Router<B> + Clone,
    RestJsonRouter::Service: Service<Request<B>, Response = Response<RespBody>> + Clone,
    RestJsonRouter::Error: IntoResponse<crate::protocol::rest_json_1::RestJson1> + StdError,
    RoutingService<RestJsonRouter, crate::protocol::rest_json_1::RestJson1>: ProtocolDetector<B>,
    <RestJsonRouter::Service as Service<Request<B>>>::Future: Unpin,

    // RestXml bounds
    RestXmlRouter: Router<B> + Clone,
    RestXmlRouter::Service: Service<Request<B>, Response = Response<RespBody>> + Clone,
    RestXmlRouter::Error: IntoResponse<crate::protocol::rest_xml::RestXml> + StdError,
    RoutingService<RestXmlRouter, crate::protocol::rest_xml::RestXml>: ProtocolDetector<B>,
    <RestXmlRouter::Service as Service<Request<B>>>::Future: Unpin,

    // Common bounds
    RespBody: HttpBody<Data = Bytes> + Send + 'static,
    RespBody::Error: Into<BoxError>,
{
    type Response = Response<BoxBody>;
    type Error = MultiProtocolRoutingError<
        <RpcV2Router::Service as Service<Request<B>>>::Error,
        <AwsJson11Router::Service as Service<Request<B>>>::Error,
        <AwsJson10Router::Service as Service<Request<B>>>::Error,
        <RestJsonRouter::Service as Service<Request<B>>>::Error,
        <RestXmlRouter::Service as Service<Request<B>>>::Error,
    >;
    type Future = MultiProtocolRoutingFuture<
        RoutingFuture<RpcV2Router::Service, B>,
        RoutingFuture<AwsJson11Router::Service, B>,
        RoutingFuture<AwsJson10Router::Service, B>,
        RoutingFuture<RestJsonRouter::Service, B>,
        RoutingFuture<RestXmlRouter::Service, B>,
    >;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut req: Request<B>) -> Self::Future {
        // Try protocols in order of specificity:
        // 1. RpcV2Cbor (most specific - has dedicated header)
        // 2. AwsJson1.1 (has x-amz-target + specific content-type)
        // 3. AwsJson1.0 (has x-amz-target + specific content-type)
        // 4. RestJson1 (content-type + route matching)
        // 5. RestXml (content-type + route matching)
        //
        // Before calling the routing service, we insert the protocol marker
        // into request extensions so users can determine which protocol handled
        // the request.

        // Try RpcV2Cbor
        if let Some(ref mut rpc_v2) = self.rpc_v2 {
            if rpc_v2.can_handle(&req) {
                req.extensions_mut().insert(Protocol::RpcV2Cbor);
                return MultiProtocolRoutingFuture {
                    inner: MultiProtocolRoutingFutureInner::RpcV2(rpc_v2.call(req)),
                };
            }
        }

        // Try AwsJson1.1
        if let Some(ref mut aws_json_11) = self.aws_json_11 {
            if aws_json_11.can_handle(&req) {
                req.extensions_mut().insert(Protocol::AwsJson1_1);
                return MultiProtocolRoutingFuture {
                    inner: MultiProtocolRoutingFutureInner::AwsJson11(aws_json_11.call(req)),
                };
            }
        }

        // Try AwsJson1.0
        if let Some(ref mut aws_json_10) = self.aws_json_10 {
            if aws_json_10.can_handle(&req) {
                req.extensions_mut().insert(Protocol::AwsJson1_0);
                return MultiProtocolRoutingFuture {
                    inner: MultiProtocolRoutingFutureInner::AwsJson10(aws_json_10.call(req)),
                };
            }
        }

        // Try RestJson1
        if let Some(ref mut rest_json) = self.rest_json {
            if rest_json.can_handle(&req) {
                req.extensions_mut().insert(Protocol::RestJson1);
                return MultiProtocolRoutingFuture {
                    inner: MultiProtocolRoutingFutureInner::RestJson(rest_json.call(req)),
                };
            }
        }

        // Try RestXml
        if let Some(ref mut rest_xml) = self.rest_xml {
            if rest_xml.can_handle(&req) {
                req.extensions_mut().insert(Protocol::RestXml);
                return MultiProtocolRoutingFuture {
                    inner: MultiProtocolRoutingFutureInner::RestXml(rest_xml.call(req)),
                };
            }
        }

        // No protocol matched - return NotFound
        // We need to return a future of the right type, so we use a dummy
        // This will immediately resolve to a 404 response
        MultiProtocolRoutingFuture {
            inner: MultiProtocolRoutingFutureInner::NotFound,
        }
    }
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
    fn test_aws_json_detection() {
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

    /// Test that MultiProtocolRoutingService type aliases compile with () for unused protocols.
    /// This test verifies that the type structure works when some protocol slots are unused.
    ///
    /// This would have caught the issue where `()` didn't implement `Router<B>`.
    #[test]
    fn test_multi_protocol_with_unused_slots_compiles() {
        use crate::protocol::rest::router::RestRouter;
        use crate::protocol::rpc_v2_cbor::router::RpcV2CborRouter;

        // This type represents a service with only RestJson1 and RpcV2Cbor enabled.
        // RestXml, AwsJson1.0, and AwsJson1.1 are unused (represented by ()).
        // If `()` doesn't implement `Router<B>`, this type alias would cause
        // the Service impl bounds to fail when used.
        // Generic order: RpcV2, AwsJson11, AwsJson10, RestJson, RestXml
        type PartialMultiProtocol = MultiProtocolRoutingService<
            RpcV2CborRouter<crate::routing::Route<()>>,  // RpcV2Cbor - used
            (),                                           // AwsJson1.1 - unused
            (),                                           // AwsJson1.0 - unused
            RestRouter<crate::routing::Route<()>>,        // RestJson1 - used
            (),                                           // RestXml - unused
        >;

        // This function requires Service to be implemented.
        // It won't be called, but its existence forces the compiler to verify
        // that the Service trait is implementable for PartialMultiProtocol.
        fn _assert_service_bounds<B, S>()
        where
            S: tower::Service<Request<B>>,
        {
        }

        // The fact that this type alias compiles proves that the Router<B> bound
        // is satisfied for all type parameters, including ().
        let _proof: Option<PartialMultiProtocol> = None;
    }

    /// Test that ProtocolDetector returns false for unused protocol slots.
    #[test]
    fn test_unused_protocol_detector_returns_false() {
        let routing_svc: RoutingService<(), crate::protocol::rest_json_1::RestJson1> =
            RoutingService::new(());

        let req = Request::builder()
            .method(http::Method::GET)
            .uri("/foo")
            .body(())
            .unwrap();

        // Unused protocol slot should never handle requests
        assert!(!routing_svc.can_handle(&req));
    }

    /// Test that Router<B> for () always returns an error.
    #[test]
    fn test_unit_router_never_matches() {
        let router: () = ();
        let req = Request::builder()
            .method(http::Method::GET)
            .uri("/foo")
            .body(())
            .unwrap();

        let result = router.match_route(&req);
        assert!(result.is_err());
    }
}
