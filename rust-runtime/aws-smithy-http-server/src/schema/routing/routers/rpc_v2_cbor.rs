/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The built-in rpcv2Cbor router: the `/service/{s}/operation/{o}` path under `smithy-protocol`.

use crate::schema::routing::RoutingError;
use http::Request;

use super::per_target;
use crate::schema::routing::{OperationIndex, ProtocolRouter, RouteClaim, RouterBuildContext, RouterBuildError};

/// Routes rpcv2Cbor on the `/service/{service}/operation/{operation}` path.
///
/// Claims a `POST` carrying `Smithy-Protocol: rpc-v2-cbor` whose path names an operation the
/// service binds. The protocol does not stream blobs: a claimed operation with a streaming blob
/// member is rejected as an unknown operation, as is a request carrying a header the protocol
/// forbids.
#[derive(Debug)]
struct RpcV2CborProtocolRouter {
    router: crate::protocol::rpc_v2_cbor::router::RpcV2CborRouter<OperationIndex>,
    /// Indexed by [`OperationIndex::index`].
    streams_blobs: Vec<bool>,
}
impl RpcV2CborProtocolRouter {
    fn unsupported(&self, target: OperationIndex) -> bool {
        self.streams_blobs[target.index]
    }
}
impl ProtocolRouter for RpcV2CborProtocolRouter {
    fn route(&self, request: &Request<()>) -> Result<OperationIndex, RoutingError> {
        use crate::routing::Router;
        match self.router.match_route(request) {
            Ok(target) if self.unsupported(target) => Err(RoutingError::unknown_operation()),
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
            Ok(target) if self.unsupported(target) => {
                RouteClaim::Rejected(RoutingError::unknown_operation())
            }
            Ok(target) => RouteClaim::Matched(target),
            Err(err @ Error::ForbiddenHeaders) => RouteClaim::Rejected(RoutingError::malformed(err)),
            Err(_) => RouteClaim::NoClaim,
        }
    }
}

/// Sizes a per-operation table to cover every target index.

pub(crate) fn rpc_v2_cbor_router(
    ctx: &RouterBuildContext<'_>,
) -> Result<impl ProtocolRouter + 'static, RouterBuildError> {
    let capitalize_routes = crate::schema::protocol::settings_bool(ctx.protocol_settings, "capitalizeRoutes")?;
    let entries = ctx.targets.iter().flat_map(|target| {
        let name = target.operation.shape_id().shape_name();
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
    let streams_blobs = per_target(
        ctx.targets,
        || false,
        |target| {
            [target.operation.input(), target.operation.output()]
                .iter()
                .any(|schema| {
                    schema
                        .members()
                        .iter()
                        .any(|member| member.streaming() && member.shape_type() == aws_smithy_schema::ShapeType::Blob)
                })
        },
    );
    Ok(RpcV2CborProtocolRouter {
        router: crate::protocol::rpc_v2_cbor::router::RpcV2CborRouter::from_owned(entries),
        streams_blobs,
    })
}

