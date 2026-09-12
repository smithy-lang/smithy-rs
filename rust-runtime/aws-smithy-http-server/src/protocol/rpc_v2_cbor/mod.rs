/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

pub mod rejection;
pub mod router;
pub mod runtime_error;

/// [Smithy RPC v2 CBOR](https://smithy.io/2.0/additional-specs/protocols/smithy-rpc-v2.html)
/// protocol.
#[derive(Debug, Default, Clone, Copy)]
pub struct RpcV2Cbor;

/// Stateful schema-driven Smithy RPC v2 CBOR protocol implementation.
#[derive(Debug)]
pub struct RpcV2CborProtocol {
    pub(crate) inner:
        crate::schema::protocol::rpc::RpcProtocol<crate::schema::protocol::rpc_v2_cbor_serde::RpcV2CborSerde>,
}

impl Default for RpcV2CborProtocol {
    fn default() -> Self {
        Self {
            inner: crate::schema::protocol::rpc::RpcProtocol::new(
                crate::schema::protocol::rpc_v2_cbor_serde::RpcV2CborSerde::default(),
                "application/cbor",
                None,
                crate::schema::protocol::rpc::RpcAccept::ModeledOutput,
                crate::schema::protocol::rpc::RpcStreaming::EventStreamContentType,
            ),
        }
    }
}
