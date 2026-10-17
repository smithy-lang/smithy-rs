/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The built-in restJson1/restXml router: `@http` method and URI, with Content-Type
//! ownership checks during strict protocol arbitration.

use crate::routing::request_spec::{PathSegment, QuerySegment, RequestSpec};
use crate::routing::Router;
use crate::schema::protocol::request::{parse_mime, payload_member, EVENT_STREAM_MIME};
use crate::schema::routing::RoutingError;
use aws_smithy_schema::ShapeType;
use http::Request;

use super::content_type_is;
use crate::schema::routing::{
    ClaimMode, MetadataProtocolRouter, OperationTarget, RouteClaim, RouterBuildContext, RouterBuildError,
};

/// Builds a REST router from the operations' HTTP method and URI bindings.
///
/// Protocol implementations outside this crate can reuse this router with their own default
/// content type and accepted aliases. Pass the context supplied to
/// [`crate::schema::MetadataRoutedProtocol::build_router`]. In strict mode, claims use each
/// input's derived content type, including payload overrides and modeled custom Content-Type
/// headers. In sole-protocol mode, claims use native method and URI routing.
#[doc(hidden)]
pub fn rest_router(
    ctx: &RouterBuildContext<'_>,
    codec_content_type: &'static str,
    codec_aliases: &'static [&'static str],
) -> Result<impl MetadataProtocolRouter + 'static, RouterBuildError> {
    let entries = ctx
        .targets
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
    let content_types = ctx
        .targets
        .iter()
        .enumerate()
        .map(|(index, target)| {
            debug_assert_eq!(target.index(), index);
            claim_content_type(target.operation().input(), codec_content_type, codec_aliases)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RestProtocolRouter {
        claim_mode: ctx.claim_mode,
        router: crate::protocol::rest::router::RestRouter::from_iter(entries),
        codec_content_type,
        content_types,
    })
}
/// Routes restJson1 and restXml on each operation's `@http` method and URI.
///
/// Claims a request whose method and path match an operation and whose present `Content-Type`
/// matches the protocol default, the payload's derived type, or a modeled custom header.
/// Inputs without body-bound members still use the protocol default for identification.
/// Event-stream inputs require their event-stream media type. Configured XML aliases apply
/// wherever the protocol default is used.
///
/// Shared payload media types and modeled custom Content-Type headers can identify both REST
/// protocols; protocol priority resolves those overlaps.
/// As the sole protocol, claims instead use native method and URI routing, leaving
/// Content-Type validation to request deserialization.
#[derive(Debug)]
struct RestProtocolRouter {
    claim_mode: ClaimMode,
    router: crate::protocol::rest::router::RestRouter<OperationTarget>,
    codec_content_type: &'static str,
    /// Indexed by [`OperationTarget::index`].
    content_types: Vec<ClaimContentType>,
}

impl MetadataProtocolRouter for RestProtocolRouter {
    fn recognizes_streaming_input(&self, request: &Request<()>) -> bool {
        if self.claim_mode == ClaimMode::SoleProtocol {
            return self.route(request).is_ok_and(|target| target.has_streaming_input());
        }
        self.router
            .match_route(request)
            .is_ok_and(|target| target.has_streaming_input() && admits(&self.content_types[target.index()], request))
    }

    fn route(&self, request: &Request<()>) -> Result<OperationTarget, RoutingError> {
        self.router.match_route(request).map_err(RoutingError::from)
    }

    fn claim(&self, request: &Request<()>) -> RouteClaim {
        if self.claim_mode == ClaimMode::SoleProtocol {
            return match self.route(request) {
                Ok(target) => RouteClaim::ClaimedWithRoute(target),
                Err(error) => RouteClaim::DeferredRejection(error),
            };
        }
        match self.router.match_route(request) {
            Ok(target) if admits(&self.content_types[target.index()], request) => RouteClaim::ClaimedWithRoute(target),
            Err(error) if content_type_is(request, self.codec_content_type) => {
                RouteClaim::DeferredRejection(error.into())
            }
            Ok(_) | Err(_) => RouteClaim::NoClaim,
        }
    }
}

/// Strict protocol identification requires a present `Content-Type`, even when the input has
/// no body-bound members and deserialization does not need to validate the header.
#[derive(Debug)]
enum ClaimContentType {
    /// An input member binds `Content-Type` itself and permits a custom value.
    CustomHeader,
    /// The protocol default or the content type derived from the payload.
    Expected(mime::Mime, &'static [&'static str]),
}

/// Derives the operation's content type for protocol identification, independently of body
/// deserialization. Without a payload override, every input uses the protocol's default.
fn claim_content_type(
    input: &aws_smithy_schema::Schema<'_>,
    codec_content_type: &'static str,
    codec_aliases: &'static [&'static str],
) -> Result<ClaimContentType, RouterBuildError> {
    if input.members().iter().any(|member| {
        member
            .http_header()
            .is_some_and(|header| header.value().eq_ignore_ascii_case("content-type"))
    }) {
        return Ok(ClaimContentType::CustomHeader);
    }
    let (content_type, aliases) = match payload_member(input) {
        Some(payload) if payload.shape_type() == ShapeType::Union && payload.streaming() => {
            return Ok(ClaimContentType::Expected(EVENT_STREAM_MIME.clone(), &[]));
        }
        Some(payload) if payload.media_type().is_some() => (payload.media_type().unwrap().value(), &[][..]),
        Some(payload) if payload.shape_type() == ShapeType::String => ("text/plain", &[][..]),
        Some(payload) if payload.shape_type() == ShapeType::Blob => ("application/octet-stream", &[][..]),
        _ => (codec_content_type, codec_aliases),
    };
    // An unparseable modeled `@mediaType` fails the build loudly: the operation could never
    // be claimed or deserialized, so the service must not start.
    parse_mime(content_type)
        .map(|mime| ClaimContentType::Expected(mime, aliases))
        .map_err(|err| RouterBuildError::Configuration(format!("{}: {err}", input.shape_id())))
}

/// Whether claiming admits this request's `Content-Type` header.
fn admits(expected: &ClaimContentType, request: &Request<()>) -> bool {
    match expected {
        ClaimContentType::CustomHeader => request.headers().contains_key(http::header::CONTENT_TYPE),
        ClaimContentType::Expected(mime, aliases) => {
            // An explicit header must match the modeled media type or an accepted
            // alias. Compare type/subtype, ignoring parameters such as charset.
            content_type_is(request, mime.essence_str()) || aliases.iter().any(|alias| content_type_is(request, alias))
        }
    }
}
