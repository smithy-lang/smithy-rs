/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

pub mod rejection;
mod route_identity;
pub mod router;
pub mod runtime_error;

/// Header identifying the Smithy RPC v2 wire format.
pub const SMITHY_PROTOCOL_HEADER: http::HeaderName = http::HeaderName::from_static("smithy-protocol");
/// The `rpcv2Cbor` protocol.
///
/// The same type is used as the protocol type parameter of generated serde servers and, with
/// `codegen.schemaSerde`, as the schema-driven protocol implementation.
pub use crate::schema::protocol::RpcV2Cbor;
