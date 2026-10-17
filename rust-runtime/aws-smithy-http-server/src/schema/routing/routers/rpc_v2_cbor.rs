/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The built-in rpcv2Cbor router: the `/service/{s}/operation/{o}` path under `smithy-protocol`.

use crate::protocol::rpc_v2_cbor::SMITHY_PROTOCOL_HEADER;
use crate::schema::routing::RoutingError;
use http::Request;

use crate::schema::routing::{
    ClaimMode, MetadataProtocolRouter, OperationTarget, RouteClaim, RouterBuildContext, RouterBuildError,
};

/// Routes rpcv2Cbor on the `/service/{service}/operation/{operation}` path.
///
/// Claims only a `POST` carrying `Smithy-Protocol: rpc-v2-cbor` whose path names this
/// service and a bound operation, as required by Smithy's identification rules.
/// The header alone can offer a deferred rejection but cannot claim a request.
/// Routing after a claim rejects forbidden headers and unsupported streaming blobs.
/// Route identity retains the legacy acceptance of URI prefixes and service namespaces;
/// configured capitalized operation aliases also participate in the known-route check.
/// As the sole protocol, claims perform native routing and defer its errors, including
/// missing or unsupported protocol headers, instead of declining protocol ownership.
#[derive(Debug)]
struct RpcV2CborProtocolRouter {
    claim_mode: ClaimMode,
    router: crate::protocol::rpc_v2_cbor::router::RpcV2CborRouter<OperationTarget>,
}
impl MetadataProtocolRouter for RpcV2CborProtocolRouter {
    fn recognizes_streaming_input(&self, request: &Request<()>) -> bool {
        if self.claim_mode == ClaimMode::SoleProtocol {
            return self.route(request).is_ok_and(|target| target.has_streaming_input());
        }
        use crate::routing::Router;
        request.method() == http::Method::POST
            && request
                .headers()
                .get(&SMITHY_PROTOCOL_HEADER)
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
        if self.claim_mode == ClaimMode::SoleProtocol {
            return match self.route(request) {
                Ok(target) => RouteClaim::ClaimedWithRoute(target),
                Err(error) => RouteClaim::DeferredRejection(error),
            };
        }
        let identified = request
            .headers()
            .get(&SMITHY_PROTOCOL_HEADER)
            .is_some_and(|value| value.as_bytes() == b"rpc-v2-cbor");
        if !identified {
            return RouteClaim::NoClaim;
        }
        if request.method() == http::Method::POST && self.router.match_target(request.uri().path()).is_some() {
            RouteClaim::Claimed
        } else {
            RouteClaim::DeferredRejection(
                self.route(request)
                    .expect_err("a wrong method or unknown target cannot route"),
            )
        }
    }
}

pub(crate) fn rpc_v2_cbor_router(
    ctx: &RouterBuildContext<'_>,
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
        names.into_iter().map(move |name| {
            (
                format!("{}/operation/{}", ctx.service.shape_id().shape_name(), name),
                *target,
            )
        })
    });
    Ok(RpcV2CborProtocolRouter {
        claim_mode: ctx.claim_mode,
        router: crate::protocol::rpc_v2_cbor::router::RpcV2CborRouter::from_owned(entries),
    })
}
