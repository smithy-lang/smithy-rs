/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The built-in rpcv2Cbor router: the `/service/{s}/operation/{o}` path under `smithy-protocol`.

use crate::schema::routing::RoutingError;
use http::Request;

use crate::schema::routing::{OperationTarget, MetadataProtocolRouter, RouteClaim, RouterBuildContext, RouterBuildError};

/// Routes rpcv2Cbor on the `/service/{service}/operation/{operation}` path.
///
/// Claims a `POST` carrying `Smithy-Protocol: rpc-v2-cbor` whose path names an operation the
/// service binds. The protocol does not stream blobs: a claimed operation with a streaming blob
/// member is rejected as an unknown operation, as is a request carrying a header the protocol
/// forbids.
#[derive(Debug)]
struct RpcV2CborProtocolRouter {
    router: crate::protocol::rpc_v2_cbor::router::RpcV2CborRouter<OperationTarget>,
}
impl MetadataProtocolRouter for RpcV2CborProtocolRouter {
    fn recognizes_streaming_input(&self, request: &Request<()>) -> bool {
        use crate::routing::Router;
        request.method() == http::Method::POST
            && request
                .headers()
                .get("smithy-protocol")
                .is_some_and(|value| value.as_bytes() == b"rpc-v2-cbor")
            && self
                .router
                .match_route(request)
                .is_ok_and(|target| target.has_streaming_input() && !target.has_streaming_blob())
    }

    fn route(&self, request: &Request<()>) -> Result<OperationTarget, RoutingError> {
        use crate::routing::Router;
        match self.router.match_route(request) {
            Ok(target) if target.has_streaming_blob() => Err(RoutingError::unknown_operation()),
            Ok(target) => Ok(target),
            Err(error) => Err(error.into()),
        }
    }

    fn claim(&self, request: &Request<()>) -> RouteClaim {
        use crate::routing::Router;
        use crate::protocol::rpc_v2_cbor::router::Error;
        let identified = request.method() == http::Method::POST
            && request
                .headers()
                .get("smithy-protocol")
                .is_some_and(|value| value.as_bytes() == b"rpc-v2-cbor");
        if !identified {
            return RouteClaim::NoClaim;
        }
        match self.router.match_route(request) {
            Ok(target) if target.has_streaming_blob() => {
                RouteClaim::Rejected(RoutingError::unknown_operation())
            }
            Ok(target) => RouteClaim::Matched(target),
            Err(err @ Error::ForbiddenHeaders) => RouteClaim::Rejected(RoutingError::malformed(err)),
            Err(_) => RouteClaim::NoClaim,
        }
    }
}

pub(crate) fn rpc_v2_cbor_router(
    ctx: &RouterBuildContext<'_>,
) -> Result<impl MetadataProtocolRouter + 'static, RouterBuildError> {
    let capitalize_routes = crate::schema::protocol::settings_bool(ctx.protocol_settings, "capitalizeRoutes")?;
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
    Ok(RpcV2CborProtocolRouter {
        router: crate::protocol::rpc_v2_cbor::router::RpcV2CborRouter::from_owned(entries),
    })
}
