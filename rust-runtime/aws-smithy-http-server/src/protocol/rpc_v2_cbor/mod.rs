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

/// [Smithy RPC v2 CBOR](https://smithy.io/2.0/additional-specs/protocols/smithy-rpc-v2.html)
/// protocol.
#[derive(Debug, Default, Clone, Copy)]
pub struct RpcV2Cbor;
