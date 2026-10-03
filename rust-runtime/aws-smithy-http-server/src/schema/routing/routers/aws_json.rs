/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The built-in awsJson router: `X-Amz-Target` on `POST /`.

use crate::schema::routing::RoutingError;
use http::Request;

use super::content_type_is;
use crate::schema::routing::{
    MetadataProtocolRouter, OperationTarget, RouteClaim, RouterBuildContext, RouterBuildError,
};

#[derive(Debug)]
struct AwsJsonProtocolRouter {
    router: crate::protocol::aws_json::router::AwsJsonRouter<OperationTarget>,
    content_type: &'static str,
}
impl MetadataProtocolRouter for AwsJsonProtocolRouter {
    fn recognizes_streaming_input(&self, request: &Request<()>) -> bool {
        request.method() == http::Method::POST
            && request.uri().path() == "/"
            && content_type_is(request, self.content_type)
            && self
                .router
                .match_target(request)
                .is_some_and(|target| target.has_streaming_input())
    }

    fn route(&self, request: &Request<()>) -> Result<OperationTarget, RoutingError> {
        use crate::routing::Router;
        self.router.match_route(request).map_err(RoutingError::from)
    }

    fn claim(&self, request: &Request<()>) -> RouteClaim {
        // The path, not the whole URI: clients may add query parameters awsJson ignores.
        if request.method() != http::Method::POST
            || request.uri().path() != "/"
            || !content_type_is(request, self.content_type)
        {
            return RouteClaim::NoClaim;
        }
        match self.router.match_target(request) {
            Some(target) => RouteClaim::ClaimedWithRoute(target),
            None => RouteClaim::NoClaim,
        }
    }
}

/// Builds the awsJson-style target router (`Service.Operation`). Exposed for out-of-tree
/// protocols that route on the same key. Among several protocols, the router claims `POST /`
/// requests whose `Content-Type` is `content_type`. Rejections are awsJson's routing errors,
/// framed by whichever protocol registers the router.
#[doc(hidden)]
pub fn aws_json_router(
    ctx: &RouterBuildContext<'_>,
    content_type: &'static str,
) -> Result<impl MetadataProtocolRouter + 'static, RouterBuildError> {
    let entries = ctx.targets.iter().map(|target| {
        let name = target.operation().shape_id().shape_name();
        (format!("{}.{}", ctx.service.shape_id().shape_name(), name), *target)
    });
    Ok(AwsJsonProtocolRouter {
        router: crate::protocol::aws_json::router::AwsJsonRouter::from_owned(entries),
        content_type,
    })
}
