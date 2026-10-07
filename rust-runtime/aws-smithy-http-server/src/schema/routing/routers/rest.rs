/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The built-in restJson1/restXml router: `@http` method and URI, content-type claims.

use crate::routing::request_spec::{PathSegment, QuerySegment, RequestSpec};
use crate::routing::Router;
use crate::schema::protocol::request::{
    expected_request_content_type, payload_member, ExpectedContentType, EVENT_STREAM_CONTENT_TYPE,
};
use crate::schema::routing::RoutingError;
use aws_smithy_schema::ShapeType;
use http::Request;

use super::content_type_is;
use crate::schema::routing::{MetadataProtocolRouter, OperationTarget, RouteClaim, RouterBuildError};

/// The `Content-Type` rules claiming applies for a request with this input.
///
/// The derivation is shared with the deserializer ([`expected_request_content_type`]) so the two
/// can never disagree: an input the deserializer reads without consulting the header (members all
/// bound to the URI or headers, a blob payload without `@mediaType`, a user-modeled empty input,
/// an input binding `Content-Type` itself with `@httpHeader`) makes no claim demand either.
///
/// Event streams are the one claim-side override: their `Content-Type` is a genuine routing
/// signal every client sends, and the buffered deserializer never sees these requests, so its
/// derivation skips the check that routing still needs.
fn claim_content_type(
    input: &aws_smithy_schema::Schema<'_>,
    codec_content_type: &'static str,
    codec_aliases: &'static [&'static str],
) -> ExpectedContentType {
    if payload_member(input).is_some_and(|payload| payload.shape_type() == ShapeType::Union && payload.streaming()) {
        return ExpectedContentType::Expect(
            EVENT_STREAM_CONTENT_TYPE
                .parse()
                .expect("the event stream media type is valid MIME"),
            &[],
        );
    }
    expected_request_content_type(input, codec_content_type, codec_aliases)
}

/// Whether claiming admits this request's `Content-Type` header.
///
/// This is the claim-specific policy over the shared derivation: `Skip` claims unconditionally,
/// and `Expect` requires a present, matching header — the deserializer's empty-body waiver does
/// not apply, because a protocol must be chosen before any body is read.
fn admits(expected: &ExpectedContentType, request: &Request<()>) -> bool {
    let present = request.headers().contains_key(http::header::CONTENT_TYPE);
    match expected {
        ExpectedContentType::Skip => true,
        // Synthetic Unit inputs retain the legacy contract that the header is absent.
        ExpectedContentType::Absent => !present,
        ExpectedContentType::Expect(mime, aliases) if present => {
            // An explicit header must match the modeled media type or an accepted
            // alias. Compare type/subtype, ignoring parameters such as charset.
            content_type_is(request, mime.essence_str()) || aliases.iter().any(|alias| content_type_is(request, alias))
        }
        ExpectedContentType::Expect(..) => false,
    }
}

/// Routes restJson1 and restXml on each operation's `@http` method and URI.
///
/// Claims a request whose method and path match an operation and whose `Content-Type` satisfies
/// the same derivation the deserializer applies ([`expected_request_content_type`]): the
/// protocol's media type when members are bound to the body, the payload's `@mediaType` or
/// derived type for an `@httpPayload`, and no demand at all when the deserializer would never
/// consult the header. Event-stream inputs require their event-stream media type. Configured XML
/// aliases are an explicit compatibility extension to the published derived-content-type rules.
/// Synthetic Unit inputs retain their legacy requirement to omit Content-Type.
///
/// A service serving both restJson1 and restXml cannot distinguish requests whose input makes no
/// Content-Type demand — URI-bound inputs, bare blob payloads, custom `@httpHeader("Content-Type")`
/// bindings, shared payload media types, or synthetic Unit inputs: the protocol earlier in
/// priority order, restJson1 unless reordered, claims them, and a client of the other protocol
/// receives a response it cannot read. Requests with a structured body carry `application/json`
/// or `application/xml` and reach the right protocol.
#[derive(Debug)]
struct RestProtocolRouter {
    router: crate::protocol::rest::router::RestRouter<OperationTarget>,
    codec_content_type: &'static str,
    /// Indexed by [`OperationTarget::index`].
    content_types: Vec<ExpectedContentType>,
}

impl MetadataProtocolRouter for RestProtocolRouter {
    fn recognizes_streaming_input(&self, request: &Request<()>) -> bool {
        self.router
            .match_route(request)
            .is_ok_and(|target| target.has_streaming_input() && admits(&self.content_types[target.index()], request))
    }

    fn route(&self, request: &Request<()>) -> Result<OperationTarget, RoutingError> {
        self.router.match_route(request).map_err(RoutingError::from)
    }

    fn claim(&self, request: &Request<()>) -> RouteClaim {
        match self.router.match_route(request) {
            Ok(target) if admits(&self.content_types[target.index()], request) => RouteClaim::ClaimedWithRoute(target),
            Err(error) if content_type_is(request, self.codec_content_type) => {
                RouteClaim::DeferredRejection(error.into())
            }
            Ok(_) | Err(_) => RouteClaim::NoClaim,
        }
    }
}

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
            claim_content_type(target.operation().input(), codec_content_type, codec_aliases)
        })
        .collect();
    Ok(RestProtocolRouter {
        router: crate::protocol::rest::router::RestRouter::from_iter(entries),
        codec_content_type,
        content_types,
    })
}
