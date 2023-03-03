/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::str::FromStr;

use http::HeaderMap;
use once_cell::sync::Lazy;
use regex::Regex;

use crate::body::empty;
use crate::body::BoxBody;
use crate::extension::RuntimeErrorExtension;
use crate::proto::aws_json_11::router::ROUTE_CUTOFF;
use crate::proto::rest::router::Error;
use crate::response::IntoResponse;
use crate::routing::tiny_map::TinyMap;
use crate::routing::Router;
use crate::routing::{method_disallowed, UNKNOWN_OPERATION_EXCEPTION};

use super::RpcV2;

pub use crate::proto::rest::router::*;

pub struct RpcV2Router<S> {
    routes: TinyMap<&'static str, S, ROUTE_CUTOFF>,
}

impl<S> RpcV2Router<S> {
    const FORBIDDEN_HEADERS: &'static [&'static str] = &["x-amz-target", "x-amzn-target"];

    fn path_regex() -> &'static Regex {
        // TODO(rpcv2): Does this regex need to be more sophisticated?
        static PATH_REGEX: Lazy<Regex> =
            Lazy::new(|| Regex::new(r#"/service/(?P<service>\w+)/operation/(?P<operation>\w+)"#).unwrap());

        &PATH_REGEX
    }

    pub fn format_regex() -> &'static Regex {
        static SMITHY_PROTOCOL_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r#"rpc-v2-(?P<format>\w+)"#).unwrap());

        &SMITHY_PROTOCOL_REGEX
    }
}

// TODO(rpcv2): Implement (current body copied from the rest xml impl)
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
        }
    }
}

/// Smithy RPC V2 requests have a `smithy-protocol` header with the value
/// `"rpc-v2-{format}"`, where `format` is one of the supported wire formats
/// by the protocol (see [`WireFormat`]).
fn get_format_from_header(headers: &HeaderMap) -> Option<WireFormat> {
    let header = headers.get("smithy-protocol")?;
    let header = header.to_str().ok()?;
    let captures = RpcV2Router::<()>::format_regex().captures(header)?;

    let format = captures.name("format")?;

    format.as_str().parse().ok()
}

/// Supported wire formats by Smithy RPC V2.
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

    // TODO(rpcv2): Implement a proper error type
    type Error = Error;

    fn match_route(&self, request: &http::Request<B>) -> Result<Self::Service, Self::Error> {
        let request_has_forbidden_header = Self::FORBIDDEN_HEADERS
            .into_iter()
            .any(|&forbidden_header| request.headers().contains_key(forbidden_header));

        if request_has_forbidden_header {
            // TODO(rpcv2): Use right variant for this error
            return Err(Error::NotFound);
        }

        // TODO(rpcv2): Use format, use a more approppriate variant for this error.
        let _wire_format = get_format_from_header(request.headers()).ok_or(Error::NotFound)?;

        let request_path = request.uri().path();
        let regex = Self::path_regex();

        let captures = regex.captures(request_path).ok_or(Error::NotFound)?;
        let (service, operation) = (&captures["service"], &captures["operation"]);

        let route_key = format!("{service}.{operation}");

        let value = self.routes.get(route_key.as_str()).ok_or(Error::NotFound)?;

        Ok(value.clone())
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

    use crate::proto::test_helpers::req;

    use super::{Router, RpcV2Router};

    #[test]
    fn path_regex_works() {
        let regex = RpcV2Router::<()>::path_regex();

        let captures = regex.captures("/service/MyService/operation/MyOperation").unwrap();
        assert_eq!("MyService", &captures["service"]);
        assert_eq!("MyOperation", &captures["operation"]);
    }

    #[test]
    fn format_regex_works() {
        let regex = RpcV2Router::<()>::format_regex();

        let captures = regex.captures("rpc-v2-something").unwrap();
        assert_eq!("something", &captures["format"]);

        let captures = regex.captures("rpc-v2-SomethingElse").unwrap();
        assert_eq!("SomethingElse", &captures["format"]);

        let invalid = regex.captures("rpc-v1-something");
        assert!(invalid.is_none());
    }

    #[test]
    fn simple_routing() {
        let routes = vec!["Service.Operation"];
        let router: RpcV2Router<_> = routes.clone().into_iter().map(|op| (op, ())).collect();
        let good_uri = "/service/Service/operation/Operation";

        // Request should match
        let mut headers = HeaderMap::new();
        headers.insert("smithy-protocol", HeaderValue::from_static("rpc-v2-cbor"));
        let routing_result = router.match_route(&req(&Method::GET, good_uri, Some(headers)));
        assert!(routing_result.is_ok());

        // The request would be valid if it had a valid `smithy-protocol` header
        let mut headers = HeaderMap::new();
        headers.insert("smithy-protocol", HeaderValue::from_static("bad-header"));
        let invalid_request_0 = &req(&Method::GET, good_uri, Some(headers));
        assert!(router.match_route(&invalid_request_0).is_err());

        // The request would be valid if it did not have the `x-amz-target` header.
        let mut headers = HeaderMap::new();
        headers.insert("smithy-protocol", HeaderValue::from_static("rpc-v2-cbor"));
        headers.insert("x-amz-target", HeaderValue::from_static("Service.Operation"));
        let invalid_request1 = req(&Method::GET, good_uri, Some(headers));
        assert!(router.match_route(&invalid_request1).is_err());

        // The request would be valid if it did not have the `x-amzn-target` header.
        let mut headers = HeaderMap::new();
        headers.insert("smithy-protocol", HeaderValue::from_static("rpc-v2-cbor"));
        headers.insert("x-amzn-target", HeaderValue::from_static("Service.Operation"));
        let invalid_request1 = req(&Method::GET, good_uri, Some(headers));
        assert!(router.match_route(&invalid_request1).is_err());
    }
}
