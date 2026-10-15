/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The built-in restJson1/restXml router: `@http` method and URI, with Content-Type
//! ownership checks during strict protocol arbitration.

use crate::routing::request_spec::{PathSegment, QuerySegment, RequestSpec};
use crate::routing::Router;
use crate::schema::protocol::request::{parse_mime, payload_member, EVENT_STREAM_MIME};
use crate::schema::request_bindings::is_body_member;
use crate::schema::routing::RoutingError;
use crate::schema::ServiceSchema;
use aws_smithy_schema::{DocumentTrait, Schema, ShapeType};
use aws_smithy_types::Document;
use http::Request;

use super::content_type_is;
use crate::schema::routing::routing_table::RoutingTable;
use crate::schema::routing::{
    MetadataProtocolRouter, OperationTarget, RouteClaim, RouterBuildContext, RouterBuildError,
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
    let operations = RoutingTable::try_new(ctx.targets, |target| {
        let input = target.operation().input();
        Ok(RestOperationMetadata {
            content_type: claim_content_type(input, codec_content_type, codec_aliases)?,
            requires_body: input.members().iter().any(|member| requires_body_member(member)),
        })
    })?;
    Ok(RestProtocolRouter {
        service_schema: ctx.service,
        router: crate::protocol::rest::router::RestRouter::from_iter(entries),
        codec_content_type,
        operations,
        default_content_type: parse_mime(codec_content_type)
            .map_err(|err| RouterBuildError::Configuration(err.to_string()))?,
    })
}
/// Routes restJson1 and restXml on each operation's `@http` method and URI.
///
/// Claims a request whose method and path match an operation and whose present `Content-Type`
/// matches the protocol default, the payload's derived type, or a modeled custom header.
/// Without Content-Type, non-streaming inputs without an explicit payload offer a deferred
/// claim unless a body member is required without a non-null default. Such a member instead
/// offers a deferred missing-content-type rejection. Required URI and header bindings do
/// not affect this decision. Input validation runs only after a claim wins arbitration.
/// When both restJson1 and restXml offer a deferred claim, protocol priority selects
/// the response protocol.
/// Event-stream inputs require their event-stream media type. Configured XML aliases apply
/// wherever the protocol default is used.
///
/// Shared payload media types and modeled custom Content-Type headers can identify both REST
/// protocols; protocol priority resolves those overlaps.
/// As the sole protocol, claims instead use native method and URI routing, leaving
/// Content-Type validation to request deserialization.
#[derive(Debug)]
struct RestProtocolRouter {
    service_schema: &'static ServiceSchema<'static>,
    router: crate::protocol::rest::router::RestRouter<OperationTarget>,
    codec_content_type: &'static str,
    operations: RoutingTable<RestOperationMetadata>,
    default_content_type: mime::Mime,
}

#[derive(Debug)]
struct RestOperationMetadata {
    content_type: ClaimContentType,
    requires_body: bool,
}

impl MetadataProtocolRouter for RestProtocolRouter {
    fn recognizes_streaming_input(&self, request: &Request<()>) -> bool {
        if self.service_schema.protocols().len() == 1 {
            return self.route(request).is_ok_and(|target| target.has_streaming_input());
        }
        self.router.match_route(request).is_ok_and(|target| {
            target.has_streaming_input()
                && admits(
                    &self
                        .operations
                        .metadata(target)
                        .expect("matched target belongs to this router")
                        .content_type,
                    request,
                )
        })
    }

    fn route(&self, request: &Request<()>) -> Result<OperationTarget, RoutingError> {
        self.router.match_route(request).map_err(RoutingError::from)
    }

    fn claim(&self, request: &Request<()>) -> RouteClaim {
        if self.service_schema.protocols().len() == 1 {
            return match self.route(request) {
                Ok(target) => RouteClaim::ClaimedWithRoute(target),
                Err(error) => RouteClaim::DeferredRejection(error),
            };
        }
        match self.router.match_route(request) {
            Ok(target)
                if admits(
                    &self
                        .operations
                        .metadata(target)
                        .expect("matched target belongs to this router")
                        .content_type,
                    request,
                ) =>
            {
                RouteClaim::ClaimedWithRoute(target)
            }
            Ok(target)
                if !request.headers().contains_key(http::header::CONTENT_TYPE)
                    && !target.has_streaming_input()
                    && payload_member(target.operation().input()).is_none() =>
            {
                if self
                    .operations
                    .metadata(target)
                    .expect("matched target belongs to this router")
                    .requires_body
                {
                    RouteClaim::DeferredRejection(RoutingError::missing_content_type(self.default_content_type.clone()))
                } else {
                    RouteClaim::DeferredClaim(target)
                }
            }
            Err(error) if content_type_is(request, self.codec_content_type) => {
                RouteClaim::DeferredRejection(error.into())
            }
            Ok(_) | Err(_) => RouteClaim::NoClaim,
        }
    }
}

/// Media types used for definite protocol identification when Content-Type is present.
#[derive(Debug)]
enum ClaimContentType {
    /// An input member binds `Content-Type` itself and permits a custom value.
    CustomHeader,
    /// The protocol default or the content type derived from the payload.
    Expected(mime::Mime, &'static [&'static str]),
}

/// Requiredness of bindings outside the body does not require a Content-Type header.
/// A null default leaves a required member unsatisfied, matching generated builders.
fn requires_body_member(member: &Schema<'_>) -> bool {
    if !is_body_member(member) {
        return false;
    }
    let Some(traits) = member.traits() else {
        return false;
    };
    traits.contains_fqn("smithy.api#required")
        && !traits
            .get_fqn("smithy.api#default")
            .and_then(|value| value.as_any().downcast_ref::<DocumentTrait>())
            .is_some_and(|value| !matches!(value.value(), Document::Null))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{OperationSchema, ServiceConfig};
    use aws_smithy_schema::{shape_id, traits::HttpTrait, AnnotationTrait, TraitMap};
    use std::sync::LazyLock;

    #[test]
    fn filtered_reordered_targets_use_matching_content_type_and_body_metadata() {
        static REQUIRED: LazyLock<TraitMap> = LazyLock::new(|| {
            let mut traits = TraitMap::new();
            traits.insert(Box::new(AnnotationTrait::new(shape_id!("smithy.api", "required"))));
            traits
        });
        static VALUE: Schema<'static> =
            Schema::new_member(shape_id!("test", "Document", "value"), ShapeType::String, "value", 0)
                .with_traits(&REQUIRED);
        static PAYLOAD: Schema<'static> =
            Schema::new_member(shape_id!("test", "Text", "value"), ShapeType::String, "value", 0).with_http_payload();
        static DOCUMENT: Schema<'static> =
            Schema::new_struct(shape_id!("test", "Document"), ShapeType::Structure, &[&VALUE])
                .with_http(HttpTrait::new("POST", "/document", Some(200)));
        static TEXT: Schema<'static> = Schema::new_struct(shape_id!("test", "Text"), ShapeType::Structure, &[&PAYLOAD])
            .with_http(HttpTrait::new("POST", "/text", Some(200)));
        static EMPTY: Schema<'static> = Schema::new_struct(shape_id!("test", "Empty"), ShapeType::Structure, &[])
            .with_http(HttpTrait::new("POST", "/empty", Some(200)));
        static DOC_OP: OperationSchema<'static> =
            OperationSchema::new(shape_id!("test", "DocumentOp"), &DOCUMENT, &EMPTY, &[]);
        static TEXT_OP: OperationSchema<'static> =
            OperationSchema::new(shape_id!("test", "TextOp"), &TEXT, &EMPTY, &[]);
        static EMPTY_OP: OperationSchema<'static> =
            OperationSchema::new(shape_id!("test", "EmptyOp"), &EMPTY, &EMPTY, &[]);
        static SERVICE: ServiceSchema<'static> = ServiceSchema::new(
            shape_id!("test", "Service"),
            None,
            &[
                shape_id!("aws.protocols", "restJson1"),
                shape_id!("aws.protocols", "restXml"),
            ],
            &[&DOC_OP, &TEXT_OP, &EMPTY_OP],
        );
        let config = ServiceConfig::default();
        let document = OperationTarget::new(7, &DOC_OP);
        let text = OperationTarget::new(usize::MAX, &TEXT_OP);
        let empty = OperationTarget::new(2, &EMPTY_OP);
        for targets in [vec![text, empty, document], vec![document, text, empty]] {
            for codec in ["application/json", "application/xml"] {
                let router = rest_router(
                    &RouterBuildContext {
                        service: &SERVICE,
                        targets: &targets,
                        config: &config,
                        protocol_settings: None,
                    },
                    codec,
                    &[],
                )
                .unwrap();
                for (path, content_type, target) in [("/document", codec, document), ("/text", "text/plain", text)] {
                    let request = Request::builder()
                        .method("POST")
                        .uri(path)
                        .header("content-type", content_type)
                        .body(())
                        .unwrap();
                    // Repeated claims borrow the same metadata without consuming it.
                    for _ in 0..2 {
                        let RouteClaim::ClaimedWithRoute(selected) = router.claim(&request) else {
                            panic!("matching content type must claim the operation");
                        };
                        assert_eq!(selected.index(), target.index());
                        assert!(std::ptr::eq(selected.operation(), target.operation()));
                    }
                    let wrong_type = Request::builder()
                        .method("POST")
                        .uri(path)
                        .header("content-type", "image/png")
                        .body(())
                        .unwrap();
                    assert!(matches!(router.claim(&wrong_type), RouteClaim::NoClaim));
                }
                let request = |path| Request::builder().method("POST").uri(path).body(()).unwrap();
                assert!(matches!(
                    router.claim(&request("/document")),
                    RouteClaim::DeferredRejection(_)
                ));
                let RouteClaim::DeferredClaim(selected) = router.claim(&request("/empty")) else {
                    panic!("empty input must defer a claim");
                };
                assert_eq!(selected.index(), empty.index());
            }
        }
    }
}
