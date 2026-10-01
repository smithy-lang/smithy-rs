/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Runtime construction and dispatch for the schema protocols a service declares.
//!
//! A service declaring one metadata protocol routes every request with that protocol's
//! [`MetadataProtocolRouter::route`]. Otherwise the protocols are asked in priority order to claim each
//! request — from the head alone; a body-routed protocol can request complete-body collection — and the request dispatches to the first that
//! claims it. When a router needs body bytes to claim, metadata routers are checked for
//! streaming inputs if the service declares any. Recognized streaming inputs skip claims
//! requiring body bytes, with no retry. Claims made from the head retain canonical priority.
//! Streaming recognition is cached for the request.
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
