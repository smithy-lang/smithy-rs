/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Runtime construction and dispatch for the schema protocols a service declares.
//!
//! Routers implement either [`MetadataProtocolRouter`], which selects operations from the
//! request URI, method and headers, or [`BodyProtocolRouter`], which can also inspect the
//! complete body. Body routers receive only operations with no streaming input or output.
//!
//! A service serving exactly one protocol with a metadata router calls
//! [`MetadataProtocolRouter::route`] directly, without a claim check. Every other configuration, including
//! a single body router, asks routers to claim in protocol priority order. The first claim
//! owns the request: it either identifies the operation or proceeds to routing, whose
//! errors are terminal and serialized by that protocol.
//!
//! Body routers first inspect the head with [`BodyProtocolRouter::claim`]. A
//! [`BodyRouteClaim::ClaimedWithRoute`] dispatches without collecting the body.
//! [`BodyRouteClaim::Claimed`] collects the body and calls [`BodyProtocolRouter::route_with_body`];
//! [`BodyRouteClaim::NeedsBodyToClaim`] collects it and calls [`BodyProtocolRouter::claim_with_body`]
//! first. Collection obeys the service's routing body limits. Collected bytes are reused
//! by later routers if the claim is declined and replayed to the selected handler.
//!
//! Before collecting for `NeedsBodyToClaim`, the service checks metadata routers for a
//! recognized streaming input, if its schema declares any. A match skips that body claim
//! without retrying it. Recognition is cached for the request. Claims already made from
//! the head retain their priority, including a body router's `Claimed` result.
//!
//! If a single body router declines, its protocol renders an unknown-operation error.
//! If every protocol in a multi-protocol service declines, the response is Coral-compatible:
//! `404` with the XML body `<UnknownOperationException/>` followed by a newline and no
//! `Content-Type` header.

mod builder;
mod protocol_router;
pub mod route_errors;
mod routers;
mod service;
#[cfg(test)]
mod tests;

pub use builder::MultiProtocolRoutingServiceBuilder;
pub use protocol_router::{
    BodyProtocolRouter, BodyRouteClaim, CollectedBody, MetadataProtocolRouter, OperationHandlerBinding,
    OperationTarget, RouteClaim, RouterBuildContext, RouterBuildError, RoutingOptions, SharedProtocolRouter,
};
pub use route_errors::{RoutingError, RoutingErrorKind};
pub use routers::aws_json_router;
pub(crate) use routers::{rest_router, rpc_v2_cbor_router};
pub use service::{MultiProtocolRoutingFuture, MultiProtocolRoutingService};
