/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The built-in awsJson router: `X-Amz-Target` on `POST /`.

use crate::schema::routing::RoutingError;
use crate::schema::ServiceSchema;
use http::Request;

use super::content_type_is;
use crate::schema::routing::{
    MetadataProtocolRouter, OperationTarget, RouteClaim, RouterBuildContext, RouterBuildError,
};

#[derive(Debug)]
struct AwsJsonProtocolRouter {
    service_schema: &'static ServiceSchema<'static>,
    router: crate::protocol::aws_json::router::AwsJsonRouter<OperationTarget>,
    content_type: &'static str,
}
impl AwsJsonProtocolRouter {
    fn matching_target(&self, request: &Request<()>) -> Option<OperationTarget> {
        // Match the operation before admitting the event-stream media type. It must
        // not make ordinary or output-only streaming operations claim this request.
        if request.method() != http::Method::POST || request.uri() != "/" {
            return None;
        }
        let target = self.router.match_target(request)?;
        (content_type_is(request, self.content_type)
            || (target.has_event_stream_input() && content_type_is(request, "application/vnd.amazon.eventstream")))
        .then_some(target)
    }
}

impl MetadataProtocolRouter for AwsJsonProtocolRouter {
    fn recognizes_streaming_input(&self, request: &Request<()>) -> bool {
        if self.service_schema.protocols().len() == 1 {
            return self.route(request).is_ok_and(|target| target.has_streaming_input());
        }
        self.matching_target(request)
            .is_some_and(|target| target.has_streaming_input())
    }

    fn route(&self, request: &Request<()>) -> Result<OperationTarget, RoutingError> {
        use crate::routing::Router;
        self.router.match_route(request).map_err(RoutingError::from)
    }

    fn claim(&self, request: &Request<()>) -> RouteClaim {
        if self.service_schema.protocols().len() == 1 {
            return match self.route(request) {
                Ok(target) => RouteClaim::ClaimedWithRoute(target),
                Err(error) => RouteClaim::DeferredRejection(error),
            };
        }
        match self.matching_target(request) {
            Some(target) => RouteClaim::ClaimedWithRoute(target),
            None if content_type_is(request, self.content_type) => match self.route(request) {
                Err(error) => RouteClaim::DeferredRejection(error),
                Ok(_) => RouteClaim::NoClaim,
            },
            None => RouteClaim::NoClaim,
        }
    }
}

/// Builds the awsJson-style target router (`Service.Operation`). Exposed for out-of-tree
/// protocols that route on the same key. Among several protocols, the router claims `POST /`
/// requests whose `Content-Type` is `content_type`, or the event-stream media type for
/// an operation with an event-stream input. Event-stream requests carry no AWS JSON
/// version marker, so the normal protocol priority resolves 1.0/1.1 ties. Rejections are awsJson's routing errors,
/// framed by whichever protocol registers the router.
/// As the sole protocol, claims use native full-URI, method, and target validation,
/// leaving Content-Type validation to request deserialization.
/// In either mode, claims use the native full-URI root check.
/// Routing always accepts the modeled operation name. The `capitalizeRoutes` protocol
/// setting additionally accepts a name with its first character uppercased.
#[doc(hidden)]
pub fn aws_json_router(
    ctx: &RouterBuildContext<'_>,
    content_type: &'static str,
) -> Result<impl MetadataProtocolRouter + 'static, RouterBuildError> {
    let capitalize_routes =
        crate::schema::settings::get::<bool>(ctx.protocol_settings, "capitalizeRoutes")?.unwrap_or(false);
    let entries = ctx.targets.iter().flat_map(|target| {
        let name = target.operation().shape_id().shape_name();
        let mut names = vec![name.to_owned()];
        if capitalize_routes {
            let mut chars = name.chars();
            if let Some(first) = chars.next() {
                let alias = format!("{}{}", first.to_uppercase(), chars.as_str());
                if alias != name {
                    names.push(alias);
                }
            }
        }
        names
            .into_iter()
            .map(move |name| (format!("{}.{}", ctx.service.shape_id().shape_name(), name), *target))
    });
    Ok(AwsJsonProtocolRouter {
        service_schema: ctx.service,
        router: crate::protocol::aws_json::router::AwsJsonRouter::from_owned(entries),
        content_type,
    })
}
