/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::convert::Infallible;
use std::str::FromStr;

use http::header::ToStrError;
use http::HeaderMap;
use nom::branch::alt;
use nom::bytes::complete::{tag, take_until, take_while, take_while1};
use nom::character::complete::satisfy;
use nom::combinator::{all_consuming, opt, recognize};
use nom::multi::fold_many0;
use nom::sequence::{pair, preceded};
use nom::IResult;
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

use super::RpcV2Cbor;

pub use crate::protocol::rest::router::*;

/// An RPC v2 CBOR routing error.
#[derive(Debug, Error)]
pub enum Error {
    /// Method was not `POST`.
    #[error("method not POST")]
    MethodNotAllowed,
    /// Requests for the `rpcv2Cbor` protocol MUST NOT contain an `x-amz-target` or `x-amzn-target`
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

/// A [`Router`] supporting the [Smithy RPC v2 CBOR] protocol.
///
/// [Smithy RPC v2 CBOR]: https://smithy.io/2.0/additional-specs/protocols/smithy-rpc-v2.html
#[derive(Debug, Clone)]
pub struct RpcV2CborRouter<S> {
    routes: TinyMap<&'static str, S, ROUTE_CUTOFF>,
}

/// Requests for the `rpcv2Cbor` protocol MUST NOT contain an `x-amz-target` or `x-amzn-target`
/// header. An `rpcv2Cbor` request is malformed if it contains either of these headers. Server-side
/// implementations MUST reject such requests for security reasons.
const FORBIDDEN_HEADERS: &[&str] = &["x-amz-target", "x-amzn-target"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RouteIdentity<'a> {
    service: &'a str,
    operation: &'a str,
    route_key: &'a str,
}

fn identifier(input: &str) -> IResult<&str, &str> {
    recognize(pair(
        alt((
            recognize(pair(
                take_while1(|character: char| character == '_'),
                satisfy(|character| character.is_ascii_alphanumeric()),
            )),
            recognize(satisfy(|character| character.is_ascii_alphabetic())),
        )),
        take_while(|character: char| character.is_ascii_alphanumeric() || character == '_'),
    ))(input)
}

fn wire_format_name(input: &str) -> IResult<&str, &str> {
    all_consuming(preceded(
        tag("rpc-v2-"),
        take_while1(|character: char| character.is_ascii_alphanumeric() || character == '_'),
    ))(input)
}

fn service_name(input: &str) -> IResult<&str, &str> {
    let (input, first_segment) = identifier(input)?;
    fold_many0(
        preceded(tag("."), identifier),
        move || first_segment,
        |_previous, segment| segment,
    )(input)
}

fn route_identity(input: &str) -> IResult<&str, RouteIdentity<'_>> {
    let route = input;
    let (input, service) = service_name(input)?;
    let service_start = route.len() - input.len() - service.len();
    let (input, _) = tag("/operation/")(input)?;
    let (input, operation) = identifier(input)?;
    Ok((
        input,
        RouteIdentity {
            service,
            operation,
            // This starts at the unqualified service name and is therefore the exact
            // `Service/operation/Operation` key emitted by server code generation.
            route_key: &route[service_start..],
        },
    ))
}

fn route_candidate(input: &str) -> IResult<&str, Option<RouteIdentity<'_>>> {
    let (input, _) = take_until("/service/")(input)?;
    let (input, _) = tag("/service/")(input)?;
    opt(all_consuming(route_identity))(input)
}

fn parse_route_identity(input: &str) -> Option<RouteIdentity<'_>> {
    // The protocol permits an arbitrary prefix. Jump between `/service/` markers without retaining
    // the prefix, then accept only a route grammar that consumes the remainder of the path. This
    // preserves the previous regex's unanchored-start and anchored-end behavior.
    let mut parser = fold_many0(route_candidate, || None, |found, candidate| candidate.or(found));
    parser(input).ok().and_then(|(_, identity)| identity)
}

impl<S> RpcV2CborRouter<S> {
    pub fn boxed<B>(self) -> RpcV2CborRouter<Route<B>>
    where
        S: Service<http::Request<B>, Response = http::Response<BoxBody>, Error = Infallible>,
        S: Send + Clone + 'static,
        S::Future: Send + 'static,
    {
        RpcV2CborRouter {
            routes: self.routes.into_iter().map(|(key, s)| (key, Route::new(s))).collect(),
        }
    }

    /// Applies a [`Layer`] uniformly to all routes.
    pub fn layer<L>(self, layer: L) -> RpcV2CborRouter<L::Service>
    where
        L: Layer<S>,
    {
        RpcV2CborRouter {
            routes: self
                .routes
                .into_iter()
                .map(|(key, route)| (key, layer.layer(route)))
                .collect(),
        }
    }
}

// TODO(https://github.com/smithy-lang/smithy/issues/2348): We're probably non-compliant here, but
// we have no tests to pin our implemenation against!
impl IntoResponse<RpcV2Cbor> for Error {
    fn into_response(self) -> http::Response<BoxBody> {
        match self {
            Error::MethodNotAllowed => method_disallowed(),
            _ => http::Response::builder()
                .status(http::StatusCode::NOT_FOUND)
                .header(http::header::CONTENT_TYPE, "application/cbor")
                .extension(RuntimeErrorExtension::new(
                    UNKNOWN_OPERATION_EXCEPTION.to_string(),
                ))
                .body(empty())
                .expect("invalid HTTP response for RPCv2 CBOR routing error; please file a bug report under https://github.com/awslabs/smithy-rs/issues"),
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
    let header = header.to_str().map_err(WireFormatError::HeaderValueNotVisibleAscii)?;
    let (_, format) = wire_format_name(header).map_err(|_| WireFormatError::HeaderValueNotValid(header.to_owned()))?;

    let wire_format_parse_res: Result<WireFormat, WireFormatFromStrError> = format.parse();
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

fn request_route_identity<B>(request: &http::Request<B>) -> Result<RouteIdentity<'_>, Error> {
    // Only `Method::POST` is allowed.
    if request.method() != http::Method::POST {
        return Err(Error::MethodNotAllowed);
    }

    // Some headers are not allowed.
    let request_has_forbidden_header = FORBIDDEN_HEADERS
        .iter()
        .any(|&forbidden_header| request.headers().contains_key(forbidden_header));
    if request_has_forbidden_header {
        return Err(Error::ForbiddenHeaders);
    }

    // Wire format has to be specified and supported.
    let _wire_format = parse_wire_format_from_header(request.headers())?;

    let request_path = request.uri().path();
    tracing::trace!(%request_path, "parsing service and operation from URI");
    let identity = parse_route_identity(request_path).ok_or(Error::NotFound)?;
    tracing::trace!(service = %identity.service, operation = %identity.operation, "parsed service and operation from URI");
    Ok(identity)
}

impl<S: Clone, B> Router<B> for RpcV2CborRouter<S> {
    type Service = S;

    type Error = Error;

    fn match_route(&self, request: &http::Request<B>) -> Result<Self::Service, Self::Error> {
        let identity = request_route_identity(request)?;
        let route = self.routes.get(identity.route_key).ok_or(Error::NotFound)?;
        Ok(route.clone())
    }
}

impl<S> FromIterator<(&'static str, S)> for RpcV2CborRouter<S> {
    #[inline]
    fn from_iter<T: IntoIterator<Item = (&'static str, S)>>(iter: T) -> Self {
        Self {
            routes: iter.into_iter().collect(),
        }
    }
}

/// Implementations retained only to compare routing strategies in benchmarks.
#[cfg(feature = "rpc-v2-cbor-router-benchmarks")]
#[doc(hidden)]
pub mod benchmarks {
    use super::*;

    /// The nom router before zero-copy route keys were introduced. It composes a
    /// `Service.Operation` string for every successful path parse.
    #[derive(Debug, Clone)]
    pub struct NomAllocatingRpcV2CborRouter<S> {
        routes: TinyMap<&'static str, S, ROUTE_CUTOFF>,
    }

    impl<S: Clone, B> Router<B> for NomAllocatingRpcV2CborRouter<S> {
        type Service = S;
        type Error = Error;

        fn match_route(&self, request: &http::Request<B>) -> Result<Self::Service, Self::Error> {
            let identity = request_route_identity(request)?;
            let route_key = format!("{}.{}", identity.service, identity.operation);
            let route = self.routes.get(route_key.as_str()).ok_or(Error::NotFound)?;
            Ok(route.clone())
        }
    }

    impl<S> FromIterator<(&'static str, S)> for NomAllocatingRpcV2CborRouter<S> {
        fn from_iter<T: IntoIterator<Item = (&'static str, S)>>(iter: T) -> Self {
            Self {
                routes: iter.into_iter().collect(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use http::{HeaderMap, HeaderValue, Method};
    use nom::combinator::all_consuming;

    use crate::protocol::test_helpers::req;

    use super::{identifier, parse_route_identity, wire_format_name, Error, RouteIdentity, Router, RpcV2CborRouter};

    #[test]
    fn valid_identifiers() {
        let valid_identifiers = ["a", "_a", "_0", "__0", "variable123", "_underscored_variable"];

        for id in valid_identifiers {
            assert!(all_consuming(identifier)(id).is_ok(), "'{id}' is incorrectly rejected");
        }
    }

    #[test]
    fn invalid_identifiers() {
        let invalid_identifiers = [
            "",
            "_",
            "0",
            "123starts_with_digit",
            "@invalid_start_character",
            " space_in_identifier",
            "invalid-character",
            "invalid@character",
            "no#hashes",
        ];

        for id in invalid_identifiers {
            assert!(all_consuming(identifier)(id).is_err(), "'{id}' is incorrectly accepted");
        }
    }

    #[test]
    fn uri_parser_accepts_valid_routes() {
        for uri in [
            "/service/Service/operation/Operation",
            "prefix/69/service/Service/operation/Operation",
            // Here the prefix is up to the last occurrence of the string `/service`.
            "prefix/69/service/Service/operation/Operation/service/Service/operation/Operation",
            // Service implementations SHOULD accept an absolute shape ID as the content of this
            // segment with the `#` character replaced with a `.` character, routing it the same as
            // if only the name was specified.
            "/service/aws.protocoltests.rpcv2Cbor.Service/operation/Operation",
            "/service/namespace.Service/operation/Operation",
        ] {
            assert_eq!(
                Some(RouteIdentity {
                    service: "Service",
                    operation: "Operation",
                    route_key: "Service/operation/Operation",
                }),
                parse_route_identity(uri),
                "uri: {uri}",
            );
        }
    }

    #[test]
    fn uri_parser_rejects_invalid_routes() {
        for uri in [
            "",
            "foo",
            "/servicee/Service/operation/Operation",
            "/service/Service",
            "/service/Service/operation/",
            "/service/Service/operation/Operation/",
            "/service/Service/operation/Operation/invalid-suffix",
            "/service/namespace.foo#Service/operation/Operation",
            "/service/namespace-Service/operation/Operation",
            "/service/.Service/operation/Operation",
            "/service/namespace./operation/Operation",
        ] {
            assert_eq!(None, parse_route_identity(uri), "uri: {uri}");
        }
    }

    #[test]
    fn wire_format_parser_works() {
        assert_eq!(Ok(("", "something")), wire_format_name("rpc-v2-something"));
        assert_eq!(Ok(("", "SomethingElse")), wire_format_name("rpc-v2-SomethingElse"));
        assert!(wire_format_name("rpc-v1-something").is_err());
        assert!(wire_format_name("rpc-v2-").is_err());
        assert!(wire_format_name("rpc-v2-cbor-suffix").is_err());
    }

    /// Helper function returning the only strictly required header.
    fn headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("smithy-protocol", HeaderValue::from_static("rpc-v2-cbor"));
        headers
    }

    #[test]
    fn simple_routing() {
        let router: RpcV2CborRouter<_> = [("Service/operation/Operation", ())].into_iter().collect();
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
            assert!(matches!(router.match_route(invalid_request), Err(Error::NotFound)));
        }

        // The request would be valid if it specified a supported wire format in the
        // `smithy-protocol` header.
        for header_name in ["bad-header", "rpc-v2-json", "foo-rpc-v2-cbor", "rpc-v2-cbor-foo"] {
            let mut headers = HeaderMap::new();
            headers.insert("smithy-protocol", HeaderValue::from_static(header_name));
            let invalid_request = &req(&Method::POST, good_uri, Some(headers));
            assert!(matches!(
                router.match_route(invalid_request),
                Err(Error::InvalidWireFormatHeader(_))
            ));
        }
    }

    #[test]
    fn route_key_borrows_the_request_path() {
        let path = "/prefix/service/namespace.Service/operation/Operation".to_string();
        let identity = parse_route_identity(&path).expect("valid route");
        assert_eq!(identity.route_key, "Service/operation/Operation");

        let path_range = path.as_ptr() as usize..path.as_ptr() as usize + path.len();
        assert!(path_range.contains(&(identity.route_key.as_ptr() as usize)));
    }
}
