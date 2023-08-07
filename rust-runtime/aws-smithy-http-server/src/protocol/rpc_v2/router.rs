/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::convert::Infallible;
use std::str::FromStr;

use http::header::ToStrError;
use http::HeaderMap;
use once_cell::sync::Lazy;
use regex::Regex;
use thiserror::Error;
use tower::Layer;
use tower::Service;

use crate::body::empty;
use crate::body::BoxBody;
use crate::extension::RuntimeErrorExtension;
use crate::protocol::aws_json_11::router::ROUTE_CUTOFF;
use crate::response::IntoResponse;
use crate::routing::tiny_map::TinyMap;
use crate::routing::Route;
use crate::routing::Router;
use crate::routing::{method_disallowed, UNKNOWN_OPERATION_EXCEPTION};

use super::RpcV2;

pub use crate::protocol::rest::router::*;

/// An RPC v2 routing error.
#[derive(Debug, Error)]
pub enum Error {
    /// Method was not `POST`.
    #[error("method not POST")]
    MethodNotAllowed,
    /// Requests for the `rpcv2` protocol MUST NOT contain an `x-amz-target` or `x-amzn-target`
    /// header.
    #[error("contains forbidden headers")]
    ForbiddenHeaders,
    /// Unable to parse `smithy-protocol` header into a valid wire format value.
    #[error("failed to parse `smithy-protocol` header into a valid wire format value")]
    InvalidWireFormatHeader(#[from] WireFormatError),
    /// Operation not found.
    #[error("operation not found")]
    NotFound,
}

// TODO Docs
#[derive(Debug, Clone)]
pub struct RpcV2Router<S> {
    routes: TinyMap<&'static str, S, ROUTE_CUTOFF>,
}

impl<S> RpcV2Router<S> {
    /// Requests for the `rpcv2` protocol MUST NOT contain an `x-amz-target` or `x-amzn-target`
    /// header. An `rpcv2` request is malformed if it contains either of these headers. Server-side
    /// implementations MUST reject such requests for security reasons.
    const FORBIDDEN_HEADERS: &'static [&'static str] = &["x-amz-target", "x-amzn-target"];

    // TODO Consider building a nom parser
    fn uri_regex() -> &'static Regex {
        // Every request for the `rpcv2` protocol MUST be sent to a URL with the following form: ``{prefix?}/service/{serviceName}/operation/{operationName}`.
        // The optional `prefix` slug may span multiple path segments and is ignored by Smithy RPC v2.
        static PATH_REGEX: Lazy<Regex> =
            Lazy::new(|| Regex::new(r#"/service/(?P<service>\w+)/operation/(?P<operation>\w+)$"#).unwrap());

        &PATH_REGEX
    }

    pub fn wire_format_regex() -> &'static Regex {
        static SMITHY_PROTOCOL_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r#"^rpc-v2-(?P<format>\w+)$"#).unwrap());

        &SMITHY_PROTOCOL_REGEX
    }

    pub fn boxed<B>(self) -> RpcV2Router<Route<B>>
    where
        S: Service<http::Request<B>, Response = http::Response<BoxBody>, Error = Infallible>,
        S: Send + Clone + 'static,
        S::Future: Send + 'static,
    {
        RpcV2Router {
            routes: self.routes.into_iter().map(|(key, s)| (key, Route::new(s))).collect(),
        }
    }

    /// Applies a [`Layer`] uniformly to all routes.
    pub fn layer<L>(self, layer: L) -> RpcV2Router<L::Service>
    where
        L: Layer<S>,
    {
        RpcV2Router {
            routes: self
                .routes
                .into_iter()
                .map(|(key, route)| (key, layer.layer(route)))
                .collect(),
        }
    }
}

// TODO: Implement (current body copied from the rest xml impl)
// and document.
/// A Smithy RPC V2 routing error.
impl IntoResponse<RpcV2> for Error {
    fn into_response(self) -> http::Response<BoxBody> {
        match self {
            Error::NotFound => http::Response::builder()
                .status(http::StatusCode::NOT_FOUND)
                .header(http::header::CONTENT_TYPE, "application/xml")
                .extension(RuntimeErrorExtension::new(
                    UNKNOWN_OPERATION_EXCEPTION.to_string(),
                ))
                .body(empty())
                .expect("invalid HTTP response for REST XML routing error; please file a bug report under https://github.com/awslabs/smithy-rs/issues"),
            Error::MethodNotAllowed => method_disallowed(),
            // TODO
            _ => todo!(),
        }
    }
}

/// Errors that can happen when parsing the wire format from the `smithy-protocol` header.
#[derive(Debug, Error)]
pub enum WireFormatError {
    /// Header not found.
    #[error("`smithy-protocol` header not found")]
    HeaderNotFound,
    /// Header value is not visible ASCII.
    #[error("`smithy-protocol` header not visible ASCII")]
    HeaderValueNotVisibleAscii(ToStrError),
    /// Header value does not match the `rpc-v2-{format}` pattern. The actual parsed header value
    /// is stored in the tuple struct.
    // https://doc.rust-lang.org/std/fmt/index.html#escaping
    #[error("`smithy-protocol` header does not match the `rpc-v2-{{format}}` pattern: `{0}`")]
    HeaderValueNotValid(String),
    /// Header value matches the `rpc-v2-{format}` pattern, but the `format` is not supported. The
    /// actual parsed header value is stored in the tuple struct.
    #[error("found unsupported `smithy-protocol` wire format: `{0}`")]
    WireFormatNotSupported(String),
}

/// Smithy RPC V2 requests have a `smithy-protocol` header with the value
/// `"rpc-v2-{format}"`, where `format` is one of the supported wire formats
/// by the protocol (see [`WireFormat`]).
fn parse_wire_format_from_header(headers: &HeaderMap) -> Result<WireFormat, WireFormatError> {
    let header = headers.get("smithy-protocol").ok_or(WireFormatError::HeaderNotFound)?;
    let header = header
        .to_str()
        .map_err(|e| WireFormatError::HeaderValueNotVisibleAscii(e))?;
    let captures = RpcV2Router::<()>::wire_format_regex()
        .captures(header)
        .ok_or_else(|| WireFormatError::HeaderValueNotValid(header.to_owned()))?;

    let format = captures
        .name("format")
        .ok_or_else(|| WireFormatError::HeaderValueNotValid(header.to_owned()))?;

    let wire_format_parse_res: Result<WireFormat, WireFormatFromStrError> = format.as_str().parse();
    wire_format_parse_res.map_err(|_| WireFormatError::WireFormatNotSupported(header.to_owned()))
}

/// Supported wire formats by RPC V2.
enum WireFormat {
    Cbor,
}

struct WireFormatFromStrError;

impl FromStr for WireFormat {
    type Err = WireFormatFromStrError;

    fn from_str(format: &str) -> Result<Self, Self::Err> {
        match format {
            "cbor" => Ok(Self::Cbor),
            _ => Err(WireFormatFromStrError),
        }
    }
}

impl<S: Clone, B> Router<B> for RpcV2Router<S> {
    type Service = S;

    type Error = Error;

    fn match_route(&self, request: &http::Request<B>) -> Result<Self::Service, Self::Error> {
        // Only `Method::POST` is allowed.
        if request.method() != http::Method::POST {
            return Err(Error::MethodNotAllowed);
        }

        // Some headers are not allowed.
        let request_has_forbidden_header = Self::FORBIDDEN_HEADERS
            .into_iter()
            .any(|&forbidden_header| request.headers().contains_key(forbidden_header));
        if request_has_forbidden_header {
            return Err(Error::ForbiddenHeaders);
        }

        // Wire format has to be specified and supported.
        let _wire_format = parse_wire_format_from_header(request.headers())?;

        // Extract the service name and the operation name from the request URI.
        let request_path = request.uri().path();
        let regex = Self::uri_regex();

        let captures = regex.captures(request_path).ok_or(Error::NotFound)?;
        let (service, operation) = (&captures["service"], &captures["operation"]);

        // Lookup in the `TinyMap` for a route for the target.
        let route = self
            .routes
            .get((format!("{service}.{operation}")).as_str())
            .ok_or(Error::NotFound)?;
        Ok(route.clone())
    }
}

impl<S> FromIterator<(&'static str, S)> for RpcV2Router<S> {
    fn from_iter<T: IntoIterator<Item = (&'static str, S)>>(iter: T) -> Self {
        Self {
            routes: iter.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use http::{HeaderMap, HeaderValue, Method};

    use crate::protocol::test_helpers::req;

    use super::{Error, Router, RpcV2Router};

    #[test]
    fn uri_regex_works() {
        let regex = RpcV2Router::<()>::uri_regex();

        for uri in [
            "/service/Service/operation/Operation",
            "prefix/69/service/Service/operation/Operation",
            // Here the prefix is up to the last occurence of the string `/service`.
            "prefix/69/service/Service/operation/Operation/service/Service/operation/Operation",
        ] {
            let captures = regex.captures(uri).unwrap();
            assert_eq!("Service", &captures["service"]);
            assert_eq!("Operation", &captures["operation"]);
        }
    }

    #[test]
    fn wire_format_regex_works() {
        let regex = RpcV2Router::<()>::wire_format_regex();

        let captures = regex.captures("rpc-v2-something").unwrap();
        assert_eq!("something", &captures["format"]);

        let captures = regex.captures("rpc-v2-SomethingElse").unwrap();
        assert_eq!("SomethingElse", &captures["format"]);

        let invalid = regex.captures("rpc-v1-something");
        assert!(invalid.is_none());
    }

    /// Helper function returning the only strictly required header.
    fn headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("smithy-protocol", HeaderValue::from_static("rpc-v2-cbor"));
        headers
    }

    #[test]
    fn simple_routing() {
        let router: RpcV2Router<_> = ["Service.Operation"].into_iter().map(|op| (op, ())).collect();
        let good_uri = "/prefix/service/Service/operation/Operation";

        // The request should match.
        let routing_result = router.match_route(&req(&Method::POST, good_uri, Some(headers())));
        assert!(routing_result.is_ok());

        // The request would be valid if it used `Method::POST`.
        let invalid_request = req(&Method::GET, good_uri, Some(headers()));
        assert!(matches!(
            router.match_route(&invalid_request),
            Err(Error::MethodNotAllowed)
        ));

        // The request would be valid if it did not have forbidden headers.
        for forbidden_header_name in ["x-amz-target", "x-amzn-target"] {
            let mut headers = headers();
            headers.insert(forbidden_header_name, HeaderValue::from_static("Service.Operation"));
            let invalid_request = req(&Method::POST, good_uri, Some(headers));
            assert!(matches!(
                router.match_route(&invalid_request),
                Err(Error::ForbiddenHeaders)
            ));
        }

        for bad_uri in [
            // These requests would be valid if they used correct URIs.
            "/prefix/Service/Service/operation/Operation",
            "/prefix/service/Service/operation/Operation/suffix",
            // These requests would be valid if their URI matched an existing operation.
            "/prefix/service/ThisServiceDoesNotExist/operation/Operation",
            "/prefix/service/Service/operation/ThisOperationDoesNotExist",
        ] {
            let invalid_request = &req(&Method::POST, bad_uri, Some(headers()));
            assert!(matches!(router.match_route(&invalid_request), Err(Error::NotFound)));
        }

        // The request would be valid if it specified a supported wire format in the
        // `smithy-protocol` header.
        for header_name in ["bad-header", "rpc-v2-json", "foo-rpc-v2-cbor", "rpc-v2-cbor-foo"] {
            let mut headers = HeaderMap::new();
            headers.insert("smithy-protocol", HeaderValue::from_static(header_name));
            let invalid_request = &req(&Method::POST, good_uri, Some(headers));
            assert!(matches!(
                router.match_route(&invalid_request),
                Err(Error::InvalidWireFormatHeader(_))
            ));
        }
    }
}
