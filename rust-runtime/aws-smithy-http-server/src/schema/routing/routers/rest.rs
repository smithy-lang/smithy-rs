/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The built-in restJson1/restXml router: `@http` method and URI, content-type claims.

use crate::routing::request_spec::{PathSegment, QuerySegment, RequestSpec};
use crate::routing::Router;
use crate::schema::routing::RoutingError;
use http::Request;

use super::content_type_is;
use crate::schema::routing::{MetadataProtocolRouter, OperationTarget, RouteClaim, RouterBuildError};

#[derive(Debug)]
enum ClaimContentType {
    /// A modeled custom header must be present, but its value does not distinguish protocols.
    Any,
    /// Synthetic Unit inputs retain the legacy contract that the header is absent.
    Absent,
    /// The header must name the derived media type or an explicitly configured alias.
    Expect(mime::Mime, &'static [&'static str]),
}

impl ClaimContentType {
    fn for_input(
        input: &aws_smithy_schema::Schema<'_>,
        codec_content_type: &'static str,
        codec_aliases: &'static [&'static str],
    ) -> Self {
        // Compatibility exception: generated operations without a modeled input
        // omit Content-Type, and the existing deserializer requires its absence.
        // Do not make these operations impossible to invoke by demanding a codec header.
        if input.members().is_empty() && input.original_name().is_none() {
            return Self::Absent;
        }
        let custom = input.members().iter().any(|member| {
            member
                .http_header()
                .is_some_and(|header| header.value().eq_ignore_ascii_case("content-type"))
        });
        if custom {
            return Self::Any;
        }
        // Claiming follows the published derived-content-type rules, independently
        // of the legacy deserializer's permissive checks for empty or blob bodies.
        let media_type = input
            .members()
            .iter()
            .copied()
            .find(|member| member.http_payload().is_some())
            .map(|payload| {
                payload.media_type().map(|media| media.value()).unwrap_or_else(|| {
                    use aws_smithy_schema::ShapeType;
                    match payload.shape_type() {
                        ShapeType::Union if payload.streaming() => "application/vnd.amazon.eventstream",
                        ShapeType::Blob => "application/octet-stream",
                        ShapeType::String => "text/plain",
                        _ => codec_content_type,
                    }
                })
            })
            .unwrap_or(codec_content_type);
        Self::Expect(
            media_type.parse().expect("modeled media types are valid MIME"),
            if media_type == codec_content_type {
                codec_aliases
            } else {
                &[]
            },
        )
    }

    fn admits(&self, request: &Request<()>) -> bool {
        let present = request.headers().contains_key(http::header::CONTENT_TYPE);
        match self {
            // The model permits a custom value, but claiming still requires the header.
            Self::Any => present,
            Self::Absent => !present,
            Self::Expect(mime, aliases) if present => {
                // An explicit header must match the modeled media type or an accepted
                // alias. Compare type/subtype, ignoring parameters such as charset.
                // Invalid or mismatched headers cannot use the empty-body fallback.
                content_type_is(request, mime.essence_str())
                    || aliases.iter().any(|alias| content_type_is(request, alias))
            }
            // A codec content type is required for claiming, even with an empty body.
            Self::Expect(..) => false,
        }
    }
}

/// Routes restJson1 and restXml on each operation's `@http` method and URI.
///
/// Claims a request whose method and path match an operation and whose `Content-Type` matches the
/// one that operation's input derives: the protocol's media type when members are bound to the
/// body or absent body, or the payload's for an `@httpPayload`. An input binding
/// `Content-Type` with `@httpHeader` permits a custom value, but still requires the header.
/// Event-stream inputs use their event-stream media type. Configured XML aliases are
/// an explicit compatibility extension to the published derived-content-type rules.
/// Synthetic Unit inputs also retain their legacy requirement to omit Content-Type.
///
/// A service serving both restJson1 and restXml cannot distinguish requests with a custom
/// Content-Type, a shared payload media type, or a synthetic Unit input:
/// the protocol earlier in priority order, restJson1 unless reordered, claims them, and a client of
/// the other protocol receives a response it cannot read. Requests with a structured body carry
/// `application/json` or `application/xml` and reach the right protocol.
#[derive(Debug)]
struct RestProtocolRouter {
    router: crate::protocol::rest::router::RestRouter<OperationTarget>,
    codec_content_type: &'static str,
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
            Err(error) if content_type_is(request, self.codec_content_type) => {
                RouteClaim::DeferredRejection(error.into())
            }
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
        codec_content_type,
        content_types,
    })
}
