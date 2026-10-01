/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Runtime construction and dispatch for the schema protocols a service declares.
//!
//! A service declaring one metadata protocol routes every request with that protocol's
//! [`MetadataProtocolRouter::route`]. Otherwise the protocols are asked in priority order to claim each
//! request — from the head alone; a body-routed protocol can request complete-body collection — and the request dispatches to the first that
//! claims it. At the first body-router boundary, a head-only check against preselected metadata
//! routers recognizes potential streaming inputs. For these requests body routers are deferred
//! until every metadata router declines, then visited once in their original order. Recognition
//! is advisory: fallback may still collect the body. Other requests retain canonical claim order.
//! A request no protocol claims is answered the way Coral answers one: `404` with
//! the XML body `<UnknownOperationException/>`.

mod contract;
pub mod route_errors;
mod routers;
mod service;
#[cfg(test)]
mod tests;

pub use contract::{
    BodyProtocolRouter, BodyRouteClaim, CollectedBody, MetadataProtocolRouter, OperationHandlerBinding,
    OperationTarget, RouteClaim, RouterBuildContext, RouterBuildError, RoutingOptions, SharedProtocolRouter,
    StreamingKind,
};
pub use route_errors::{RoutingError, RoutingErrorKind};
pub use routers::aws_json_router;
pub(crate) use routers::{rest_router, rpc_v2_cbor_router};
pub use service::{MultiProtocolRoutingFuture, MultiProtocolRoutingService};
