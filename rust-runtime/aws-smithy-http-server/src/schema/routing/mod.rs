/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Runtime construction and dispatch for the schema protocols a service declares.
//!
//! A service declaring one metadata protocol routes every request with that protocol's
//! [`ProtocolRouter::route`]. Otherwise the protocols are asked in priority order to claim each
//! request — from the head alone; a body-routed protocol escalates by returning a
//! [`BodyRequirement`] the service satisfies — and the request dispatches to the first that
//! claims it. A request no protocol claims is answered the way Coral answers one: `404` with
//! the XML body `<UnknownOperationException/>`.

mod collect;
mod contract;
pub mod route_errors;
mod routers;
mod service;
#[cfg(test)]
mod tests;

pub use contract::{
    BodyProtocolRouter, BodyRequirement, BodyRouteClaim, ClaimDecoder, CollectedBody,
    OperationHandlerBinding, OperationIndex, ProtocolRouter, RouteClaim, RouterBuildContext,
    RouterBuildError, RoutingOptions, SharedProtocolRouter,
};
pub use route_errors::{RoutingError, RoutingErrorKind};
pub use routers::aws_json_router;
pub(crate) use routers::{rest_router, rpc_v2_cbor_router};
pub use service::{MultiProtocolRoutingFuture, MultiProtocolRoutingService};
