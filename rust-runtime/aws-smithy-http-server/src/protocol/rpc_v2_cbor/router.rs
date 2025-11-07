/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::convert::Infallible;
use std::str::FromStr;
use std::sync::LazyLock;

use crate::http;
use crate::http::header::ToStrError;
use crate::http::HeaderMap;
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

/// Matches the `Identifier` ABNF rule in
/// <https://smithy.io/2.0/spec/model.html#shape-id-abnf>.
const IDENTIFIER_PATTERN: &str = r#"((_+([A-Za-z]|[0-9]))|[A-Za-z])[A-Za-z0-9_]*"#;

impl<S> RpcV2CborRouter<S> {
    // TODO(https://github.com/smithy-lang/smithy-rs/issues/3748) Consider building a nom parser.
    fn uri_path_regex() -> &'static Regex {
        // Every request for the `rpcv2Cbor` protocol MUST be sent to a URL with the
        // following form: `{prefix?}/service/{serviceName}/operation/{operationName}`
        //
        // * The optional `prefix` segment may span multiple path segments and is not
        //   utilized by the Smithy RPC v2 CBOR protocol. For example, a service could
        //   use a `v1` prefix for the following URL path: `v1/service/FooService/operation/BarOperation`
        // * The `serviceName` segment MUST be replaced by the [`shape
        //   name`](https://smithy.io/2.0/spec/model.html#grammar-token-smithy-Identifier)
        //   of the service's [Shape ID](https://smithy.io/2.0/spec/model.html#shape-id)
        //   in the Smithy model. The `serviceName` produced by client implementations
        //   MUST NOT contain the namespace of the `service` shape. Service
        //   implementations SHOULD accept an absolute shape ID as the content of this
        //   segment with the `#` character replaced with a `.` character, routing it
        //   the same as if only the name was specified. For example, if the `service`'s
        //   absolute shape ID is `com.example#TheService`, a service should accept both
        //   `TheService` and `com.example.TheService` as values for the `serviceName`
        //   segment.
        static PATH_REGEX: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(&format!(
                r#"/service/({IDENTIFIER_PATTERN}\.)*(?P<service>{IDENTIFIER_PATTERN})/operation/(?P<operation>{IDENTIFIER_PATTERN})$"#,
            ))
            .unwrap()
        });

        &PATH_REGEX
    }

    pub fn wire_format_regex() -> &'static Regex {
        static SMITHY_PROTOCOL_REGEX: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r#"^rpc-v2-(?P<format>\w+)$"#).unwrap());

        &SMITHY_PROTOCOL_REGEX
    }

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
    let captures = RpcV2CborRouter::<()>::wire_format_regex()
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

impl<S: Clone, B> Router<B> for RpcV2CborRouter<S> {
    type Service = S;

    type Error = Error;

    fn match_route(&self, request: &http::Request<B>) -> Result<Self::Service, Self::Error> {
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

        // Extract the service name and the operation name from the request URI.
        let request_path = request.uri().path();
        let regex = Self::uri_path_regex();

        tracing::trace!(%request_path, "capturing service and operation from URI");
        let captures = regex.captures(request_path).ok_or(Error::NotFound)?;
        let (service, operation) = (&captures["service"], &captures["operation"]);
        tracing::trace!(%service, %operation, "captured service and operation from URI");

        // Lookup in the `TinyMap` for a route for the target.
        let route = self
            .routes
            .get((format!("{service}.{operation}")).as_str())
            .ok_or(Error::NotFound)?;
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

#[cfg(test)]
mod tests {
    use crate::http::{HeaderMap, HeaderValue, Method};
    use regex::Regex;

    use crate::protocol::test_helpers::req;

    use super::{Error, Router, RpcV2CborRouter};

    fn identifier_regex() -> Regex {
        Regex::new(&format!("^{}$", super::IDENTIFIER_PATTERN)).unwrap()
    }

    #[test]
    fn valid_identifiers() {
        let valid_identifiers = vec!["a", "_a", "_0", "__0", "variable123", "_underscored_variable"];

        for id in &valid_identifiers {
            assert!(identifier_regex().is_match(id), "'{id}' is incorrectly rejected");
        }
    }

    #[test]
    fn invalid_identifiers() {
        let invalid_identifiers = vec![
            "0",
            "123starts_with_digit",
            "@invalid_start_character",
            " space_in_identifier",
            "invalid-character",
            "invalid@character",
            "no#hashes",
        ];

        for id in &invalid_identifiers {
            assert!(!identifier_regex().is_match(id), "'{id}' is incorrectly accepted");
        }
    }

    #[test]
    fn uri_regex_works_accepts() {
        let regex = RpcV2CborRouter::<()>::uri_path_regex();

        for uri in [
            "/service/Service/operation/Operation",
            "prefix/69/service/Service/operation/Operation",
            // Here the prefix is up to the last occurrence of the string `/service`.
            "prefix/69/service/Service/operation/Operation/service/Service/operation/Operation",
            // Service implementations SHOULD accept an absolute shape ID as the content of this
            // segment with the `#` character replaced with a `.` character, routing it the same as
            // if only the name was specified. For example, if the `service`'s absolute shape ID is
            // `com.example#TheService`, a service should accept both `TheService` and
            // `com.example.TheService` as values for the `serviceName` segment.
            "/service/aws.protocoltests.rpcv2Cbor.Service/operation/Operation",
            "/service/namespace.Service/operation/Operation",
        ] {
            let captures = regex.captures(uri).unwrap();
            assert_eq!("Service", &captures["service"], "uri: {uri}");
            assert_eq!("Operation", &captures["operation"], "uri: {uri}");
        }
    }

    #[test]
    fn uri_regex_works_rejects() {
        let regex = RpcV2CborRouter::<()>::uri_path_regex();

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
        ] {
            assert!(regex.captures(uri).is_none(), "uri: {uri}");
        }
    }

    #[test]
    fn wire_format_regex_works() {
        let regex = RpcV2CborRouter::<()>::wire_format_regex();

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
        let router: RpcV2CborRouter<_> = ["Service.Operation"].into_iter().map(|op| (op, ())).collect();
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
}
