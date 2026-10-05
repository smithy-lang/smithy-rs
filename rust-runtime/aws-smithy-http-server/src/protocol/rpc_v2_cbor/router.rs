/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::convert::Infallible;
use std::str::FromStr;
use std::sync::LazyLock;

use http::header::ToStrError;
use http::HeaderMap;
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

use super::route_identity::parse_route_identity;
use super::{RpcV2Cbor, SMITHY_PROTOCOL_HEADER};

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
/// Register routes using `Service/operation/Operation` keys. Request lookup borrows
/// this tail directly from the URI, ignoring any prefix and service namespace.
///
/// [Smithy RPC v2 CBOR]: https://smithy.io/2.0/additional-specs/protocols/smithy-rpc-v2.html
#[derive(Debug, Clone)]
pub struct RpcV2CborRouter<S> {
    routes: TinyMap<std::borrow::Cow<'static, str>, S, ROUTE_CUTOFF>,
}

/// Requests for the `rpcv2Cbor` protocol MUST NOT contain an `x-amz-target` or `x-amzn-target`
/// header. An `rpcv2Cbor` request is malformed if it contains either of these headers. Server-side
/// implementations MUST reject such requests for security reasons.
const FORBIDDEN_HEADERS: &[&str] = &["x-amz-target", "x-amzn-target"];

impl<S> RpcV2CborRouter<S> {
    /// Builds routing keys owned by a runtime service schema adapter.
    pub fn from_owned(iter: impl IntoIterator<Item = (String, S)>) -> Self {
        Self {
            routes: iter
                .into_iter()
                .map(|(key, value)| (std::borrow::Cow::Owned(key), value))
                .collect(),
        }
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
    let header = headers
        .get(&SMITHY_PROTOCOL_HEADER)
        .ok_or(WireFormatError::HeaderNotFound)?;
    let header = header.to_str().map_err(WireFormatError::HeaderValueNotVisibleAscii)?;
    // `to_str` excludes non-ASCII bytes, so ASCII alphanumerics and `_` are
    // equivalent to the regex's `\w` here. Check the prefix at the start and
    // every suffix byte to preserve the whole-string `^rpc-v2-(\w+)$` match.
    let format = header
        .strip_prefix("rpc-v2-")
        .filter(|format| !format.is_empty() && format.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
        .ok_or_else(|| WireFormatError::HeaderValueNotValid(header.to_owned()))?;

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

        let request_path = request.uri().path();
        tracing::trace!(%request_path, "parsing service and operation from URI");
        let identity = parse_route_identity(request_path).ok_or(Error::NotFound)?;
        tracing::trace!(service = %identity.service, operation = %identity.operation, "parsed service and operation from URI");

        // The route key is a slice of the request path; no intermediate String is needed.
        let route = self.routes.get(identity.route_key).ok_or(Error::NotFound)?;
        Ok(route.clone())
    }
}

impl<S> FromIterator<(&'static str, S)> for RpcV2CborRouter<S> {
    #[inline]
    fn from_iter<T: IntoIterator<Item = (&'static str, S)>>(iter: T) -> Self {
        Self {
            routes: iter
                .into_iter()
                .map(|(key, value)| (std::borrow::Cow::Borrowed(key), value))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_route_identity, SMITHY_PROTOCOL_HEADER};
    use crate::protocol::rpc_v2_cbor::route_identity::{has_valid_identifier_start, is_word, RouteIdentity};
    use http::{HeaderMap, HeaderValue, Method};

    use crate::protocol::test_helpers::req;

    use super::{parse_wire_format_from_header, Error, Router, RpcV2CborRouter, WireFormatError};

    fn is_valid_identifier(identifier: &str) -> bool {
        identifier.as_bytes().iter().copied().all(is_word) && has_valid_identifier_start(identifier.as_bytes())
    }

    #[test]
    fn valid_identifiers() {
        let valid_identifiers = ["a", "_a", "_0", "__0", "variable123", "_underscored_variable"];
        for id in valid_identifiers {
            assert!(is_valid_identifier(id), "'{id}' is incorrectly rejected");
            assert!(
                is_valid_identifier(&id.to_uppercase()),
                "'{id}' is incorrectly rejected"
            );
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
            assert!(!is_valid_identifier(id), "'{id}' is incorrectly accepted");
            assert!(
                !is_valid_identifier(&id.to_uppercase()),
                "'{id}' is incorrectly accepted"
            );
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
            "🦀/prefix/service/Service/operation/Operation",
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
        for (uri, service, operation, route_key) in [
            ("/service/a/operation/b", "a", "b", "a/operation/b"),
            ("/service/_a/operation/b", "_a", "b", "_a/operation/b"),
            ("/service/a___/operation/b", "a___", "b", "a___/operation/b"),
            ("/service/_a_/operation/b", "_a_", "b", "_a_/operation/b"),
            ("/service/a/operation/b_", "a", "b_", "a/operation/b_"),
            ("/service/a/operation/_b", "a", "_b", "a/operation/_b"),
            ("/service/com.x._a/operation/b", "_a", "b", "_a/operation/b"),
        ] {
            assert_eq!(
                Some(RouteIdentity {
                    service,
                    operation,
                    route_key,
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
            "/",
            "/servicee/operation/Operation",
            "/service/operation/",
            "/service//operation/",
            "/service//operation/a",
            "/service/operation",
            "/service/a/Operation/b",
            "/Service/a/operation/b",
            "service/operation",
            "/servicee/Service/operation/Operation",
            "/service/Service",
            "/service/Service/operation/",
            "/service/Service/operation/Operation/",
            "/service/Service/operation/Operation/invalid-suffix",
            "/service+Service/operation/Operation",
            "/service.Service/operation/Operation",
            "/serviceAService/operation/Operation",
            "/service0Service/operation/Operation",
            "/service-Service/operation/Operation",
            "/service=Service/operation/Operation",
            "/service/namespace.foo#Service/operation/Operation",
            "/service/namespace-Service/operation/Operation",
            "/service/.Service/operation/Operation",
            "/service/._Service/operation/Operation",
            "/service/namespace./operation/Operation",
            "/service/Sérvice/operation/Operation",
            "/service/Service/operation/Opération",
            "/service/Service/operation/Operation%20",
            "prefix/service/namespace./operation/Operation",
            "prefix/69/service/namespace./operation/Operation",
        ] {
            assert_eq!(None, parse_route_identity(uri), "uri: {uri}");
        }
    }

    #[test]
    fn wire_format_error_classification_is_preserved() {
        assert!(matches!(
            parse_wire_format_from_header(&HeaderMap::new()),
            Err(WireFormatError::HeaderNotFound)
        ));
        for (value, malformed) in [
            ("rpc-v2-json", false),
            ("rpc-v2-sparrowhawk", false),
            ("rpc-v2-CBOR", false),
            ("rpc-v2-Az_09", false),
            ("rpc-v2-_", false),
            ("rpc-v2-", true),
            ("rpc-v2-cbor-suffix", true),
            ("prefix-rpc-v2-cbor", true),
            (" rpc-v2-cbor", true),
            ("rpc-v2-cbor ", true),
            ("rpc-v2-cbor\t", true),
            ("RPC-v2-cbor", true),
            ("not-rpc-v2", true),
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(SMITHY_PROTOCOL_HEADER, HeaderValue::from_static(value));
            let error = parse_wire_format_from_header(&headers)
                .err()
                .expect("invalid wire format");
            if malformed {
                assert!(matches!(error, WireFormatError::HeaderValueNotValid(_)), "{value}");
            } else {
                assert!(matches!(error, WireFormatError::WireFormatNotSupported(_)), "{value}");
            }
        }
        let mut headers = HeaderMap::new();
        headers.insert(SMITHY_PROTOCOL_HEADER, HeaderValue::from_bytes(b"rpc-v2-\xff").unwrap());
        assert!(matches!(
            parse_wire_format_from_header(&headers),
            Err(WireFormatError::HeaderValueNotVisibleAscii(_))
        ));
    }

    #[test]
    fn hand_wire_format_parser_matches_regex() {
        // Keep the previous parser as an oracle, including its error variants and payloads.
        fn regex_parse(headers: &HeaderMap) -> Result<super::WireFormat, WireFormatError> {
            let header = headers
                .get(&SMITHY_PROTOCOL_HEADER)
                .ok_or(WireFormatError::HeaderNotFound)?;
            let header = header.to_str().map_err(WireFormatError::HeaderValueNotVisibleAscii)?;
            let captures = RpcV2CborRouter::<()>::wire_format_regex()
                .captures(header)
                .ok_or_else(|| WireFormatError::HeaderValueNotValid(header.to_owned()))?;
            let format = captures
                .name("format")
                .ok_or_else(|| WireFormatError::HeaderValueNotValid(header.to_owned()))?;
            format
                .as_str()
                .parse()
                .map_err(|_| WireFormatError::WireFormatNotSupported(header.to_owned()))
        }
        let compare = |headers: &HeaderMap| {
            let expected = regex_parse(headers).map(|_| ()).map_err(|err| format!("{err:?}"));
            let actual = parse_wire_format_from_header(headers)
                .map(|_| ())
                .map_err(|err| format!("{err:?}"));
            assert_eq!(actual, expected, "{headers:?}");
        };
        compare(&HeaderMap::new());
        let check = |bytes: &[u8]| {
            if let Ok(value) = http::HeaderValue::from_bytes(bytes) {
                let mut headers = HeaderMap::new();
                headers.insert(SMITHY_PROTOCOL_HEADER, value);
                compare(&headers);
            }
        };
        check(b"rpc-v2-cbor");
        for byte in 0..=255u8 {
            // Prefix mutations exercise the start anchor; suffixes exercise the
            // end anchor, word characters, whitespace, and non-ASCII rejection.
            for position in 0..b"rpc-v2-cbor".len() {
                let mut bytes = *b"rpc-v2-cbor";
                bytes[position] = byte;
                check(&bytes);
            }
            check(&[b"rpc-v2-cbor".as_slice(), &[byte]].concat());
            for second in 0..=255u8 {
                check(&[b"rpc-v2-".as_slice(), &[byte, second]].concat());
            }
        }
    }

    #[test]
    fn owned_routes_match_uri_tails_in_both_map_modes() {
        for count in [1, super::ROUTE_CUTOFF + 1] {
            let router =
                RpcV2CborRouter::from_owned((0..count).map(|i| (format!("Service/operation/Operation{i}"), i)));
            for i in 0..count {
                for path in [
                    format!("/service/Service/operation/Operation{i}"),
                    format!("/prefix/service/com.example.Service/operation/Operation{i}?ignored=/service/Other/operation/Other"),
                ] {
                    assert_eq!(router.match_route(&req(&Method::POST, &path, Some(headers()))).unwrap(), i);
                }
            }
            for path in [
                "/service/Other/operation/Operation0",
                "/service/Service/operation/operation0",
                "/service/Service/operation/Operation0/",
                "/service/Service/operation/Operation999999",
            ] {
                assert!(
                    matches!(
                        router.match_route(&req(&Method::POST, path, Some(headers()))),
                        Err(Error::NotFound)
                    ),
                    "{path}"
                );
            }
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
        headers.insert(SMITHY_PROTOCOL_HEADER, HeaderValue::from_static("rpc-v2-cbor"));
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
            headers.insert(SMITHY_PROTOCOL_HEADER, HeaderValue::from_static(header_name));
            let invalid_request = &req(&Method::POST, good_uri, Some(headers));
            assert!(matches!(
                router.match_route(invalid_request),
                Err(Error::InvalidWireFormatHeader(_))
            ));
        }
    }

    fn legacy_path_regex() -> &'static regex::Regex {
        const IDENTIFIER: &str = r#"((_+([A-Za-z]|[0-9]))|[A-Za-z])[A-Za-z0-9_]*"#;
        static REGEX: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
            regex::Regex::new(&format!(
                r#"/service/({IDENTIFIER}\.)*(?P<service>{IDENTIFIER})/operation/(?P<operation>{IDENTIFIER})$"#,
            ))
            .expect("valid legacy regex")
        });
        &REGEX
    }

    fn legacy_parse(path: &str) -> Option<(&str, &str)> {
        let captures = legacy_path_regex().captures(path)?;
        Some((captures.name("service")?.as_str(), captures.name("operation")?.as_str()))
    }

    struct DeterministicGenerator(u64);

    impl DeterministicGenerator {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0 >> 33
        }
    }

    #[test]
    fn handwritten_parser_matches_legacy_regex() {
        const ALPHABET: &[u8] = b"aZ09_./-#soperatinvc";
        const FRAGMENTS: &[&str] = &[
            "/service/",
            "/operation/",
            "service/",
            "operation/",
            "operation",
            "com.example.",
            "Foo",
            "_",
            "__",
            ".",
            "/",
            "Bar_1",
            "-",
            "#",
        ];
        // Cross valid and malformed segments so equivalence checks exercise successful
        // parsing as well as the mostly-invalid arbitrary corpus below.
        for prefix in ["", "/prefix", "/service/Old/operation/Old", "🦀"] {
            for service in [
                "Service",
                "_0",
                "__",
                "com.example.Service",
                ".Service",
                "com..Service",
                "Sérvice",
            ] {
                for operation in [
                    "Operation",
                    "_0",
                    "__",
                    "0bad",
                    "bad-name",
                    "",
                    "Operation/",
                    "Operation%20",
                ] {
                    let path = format!("{prefix}/service/{service}/operation/{operation}");
                    let actual = parse_route_identity(&path).map(|identity| (identity.service, identity.operation));
                    assert_eq!(legacy_parse(&path), actual, "parser/regex mismatch on {path:?}");
                }
            }
        }
        let mut generator = DeterministicGenerator(0x5EED);
        for iteration in 0..100_000 {
            let mut path = String::new();
            if iteration % 2 == 0 {
                for _ in 0..(generator.next() % 60) {
                    path.push(ALPHABET[generator.next() as usize % ALPHABET.len()] as char);
                }
            } else {
                for _ in 0..(generator.next() % 8) {
                    path.push_str(FRAGMENTS[generator.next() as usize % FRAGMENTS.len()]);
                }
            }

            let expected = legacy_parse(&path);
            let actual = parse_route_identity(&path).map(|identity| (identity.service, identity.operation));
            assert_eq!(expected, actual, "parser/regex mismatch on input {path:?}");
        }
    }

    #[test]
    fn route_key_borrows_the_request_path() {
        let path = "/prefix/service/namespace.Service/operation/Operation".to_string();
        let identity = parse_route_identity(&path).expect("valid route");
        assert_eq!(identity.route_key, "Service/operation/Operation");

        // The borrowed pointer must be within the path's memory range.
        let path_range = path.as_ptr() as usize..path.as_ptr() as usize + path.len();
        assert!(path_range.contains(&(identity.route_key.as_ptr() as usize)));
    }
}
