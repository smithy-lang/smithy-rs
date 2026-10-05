/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The built-in restJson1/restXml router: `@http` method and URI, content-type claims.

use crate::routing::request_spec::{PathSegment, QuerySegment, RequestSpec};
use crate::routing::Router;
use crate::schema::protocol::request::{expected_request_content_type, ExpectedContentType};
use crate::schema::routing::RoutingError;
use http::Request;

use super::{announces_no_body, content_type_is};
use crate::schema::routing::{MetadataProtocolRouter, OperationTarget, RouteClaim, RouterBuildError};

#[derive(Debug)]
enum ClaimContentType {
    /// The header does not tell the protocol apart; method and path decide.
    Any,
    /// The header must be absent.
    Absent,
    /// The header must name this media type or one of the aliases the protocol accepts for it, or
    /// be absent with an empty body.
    Expect(mime::Mime, &'static [&'static str]),
}

impl ClaimContentType {
    fn for_input(
        input: &aws_smithy_schema::Schema<'_>,
        codec_content_type: &'static str,
        codec_aliases: &'static [&'static str],
    ) -> Self {
        let custom = input.members().iter().any(|member| {
            member
                .http_header()
                .is_some_and(|header| header.value().eq_ignore_ascii_case("content-type"))
        });
        if custom {
            return Self::Any;
        }
        match expected_request_content_type(input, codec_content_type, codec_aliases) {
            ExpectedContentType::Skip => Self::Any,
            ExpectedContentType::Absent => Self::Absent,
            ExpectedContentType::Expect(mime, aliases) => Self::Expect(mime, aliases),
        }
    }

    fn admits(&self, request: &Request<()>) -> bool {
        let present = request.headers().contains_key(http::header::CONTENT_TYPE);
        match self {
            // The model gives us no content-type restriction for choosing a protocol.
            Self::Any => true,
            // The modeled input requires the header to be omitted. An empty or
            // malformed header still counts as present and therefore fails this check.
            Self::Absent => !present,
            Self::Expect(mime, aliases) if present => {
                // An explicit header must match the modeled media type or an accepted
                // alias. Compare type/subtype, ignoring parameters such as charset.
                // Invalid or mismatched headers cannot use the empty-body fallback.
                content_type_is(request, mime.essence_str())
                    || aliases.iter().any(|alias| content_type_is(request, alias))
            }
            // Without Content-Type, allow a request whose headers announce no body:
            // no Transfer-Encoding, and Content-Length either absent or exactly "0".
            // This is a routing decision based on the request head, not a body check.
            Self::Expect(..) => announces_no_body(request),
        }
    }
}

/// Routes restJson1 and restXml on each operation's `@http` method and URI.
///
/// Claims a request whose method and path match an operation and whose `Content-Type` matches the
/// one that operation's input derives: the protocol's media type when members are bound to the
/// body, the payload's for an `@httpPayload`, none for an input without members. An input binding
/// `Content-Type` with `@httpHeader`, or one whose content type the header cannot distinguish (a
/// streaming or untyped blob payload, members bound only to the URI and headers), is claimed on
/// method and path alone; so is a request with neither `Content-Type` nor a body.
///
/// A service serving both restJson1 and restXml therefore cannot tell them apart for such requests:
/// the protocol earlier in priority order, restJson1 unless reordered, claims them, and a client of
/// the other protocol receives a response it cannot read. Requests with a structured body carry
/// `application/json` or `application/xml` and reach the right protocol.
#[derive(Debug)]
struct RestProtocolRouter {
    router: crate::protocol::rest::router::RestRouter<OperationTarget>,
    /// Indexed by [`OperationTarget::index`].
    content_types: Vec<ClaimContentType>,
}

impl MetadataProtocolRouter for RestProtocolRouter {
    fn recognizes_streaming_input(&self, request: &Request<()>) -> bool {
        self.router
            .match_route(request)
            .is_ok_and(|target| target.has_streaming_input() && self.content_types[target.index()].admits(request))
    }

    fn route(&self, request: &Request<()>) -> Result<OperationTarget, RoutingError> {
        self.router.match_route(request).map_err(RoutingError::from)
    }

    fn claim(&self, request: &Request<()>) -> RouteClaim {
        match self.router.match_route(request) {
            Ok(target) if self.content_types[target.index()].admits(request) => RouteClaim::ClaimedWithRoute(target),
            Ok(_) | Err(_) => RouteClaim::NoClaim,
        }
    }
}

/// Routes awsJson1.0 and awsJson1.1 on `X-Amz-Target`.
///
/// Claims a `POST` to the path `/` whose `Content-Type` is the protocol's media type and whose
/// `X-Amz-Target` names an operation the service binds.

pub(crate) fn rest_router(
    targets: &[OperationTarget],
    codec_content_type: &'static str,
    codec_aliases: &'static [&'static str],
) -> Result<impl MetadataProtocolRouter + 'static, RouterBuildError> {
    let entries = targets
        .iter()
        .map(|target| {
            let http = target.operation().http().ok_or_else(|| {
                RouterBuildError::Configuration(format!("missing HTTP trait on {}", target.operation().shape_id()))
            })?;
            let method = http
                .method()
                .parse()
                .map_err(|err| RouterBuildError::Protocol(Box::new(err)))?;
            let (path, query) = http.uri().split_once('?').unwrap_or((http.uri(), ""));
            let segments = path
                .trim_start_matches('/')
                .split('/')
                .filter(|s| !s.is_empty())
                .map(|segment| {
                    if segment.starts_with('{') && segment.ends_with("+}") {
                        PathSegment::Greedy
                    } else if segment.starts_with('{') && segment.ends_with('}') {
                        PathSegment::Label
                    } else {
                        PathSegment::Literal(segment.to_owned())
                    }
                })
                .collect();
            let query = form_urlencoded::parse(query.as_bytes())
                .map(|(key, value)| {
                    if value.is_empty() {
                        QuerySegment::Key(key.into_owned())
                    } else {
                        QuerySegment::KeyValue(key.into_owned(), value.into_owned())
                    }
                })
                .collect();
            Ok((
                RequestSpec::new(
                    method,
                    crate::routing::request_spec::UriSpec::new(crate::routing::request_spec::PathAndQuerySpec::new(
                        crate::routing::request_spec::PathSpec::from_vector_unchecked(segments),
                        crate::routing::request_spec::QuerySpec::from_vector_unchecked(query),
                    )),
                ),
                *target,
            ))
        })
        .collect::<Result<Vec<_>, RouterBuildError>>()?;
    // REST receives all targets in handler order, including streaming operations.
    let content_types = targets
        .iter()
        .enumerate()
        .map(|(index, target)| {
            debug_assert_eq!(target.index(), index);
            ClaimContentType::for_input(target.operation().input(), codec_content_type, codec_aliases)
        })
        .collect();
    Ok(RestProtocolRouter {
        router: crate::protocol::rest::router::RestRouter::from_iter(entries),
        content_types,
    })
}
