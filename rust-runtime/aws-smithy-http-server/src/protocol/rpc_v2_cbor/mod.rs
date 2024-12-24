/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

pub mod rejection;
pub mod router;
pub mod runtime_error;

/// [Smithy RPC v2 CBOR](https://smithy.io/2.0/additional-specs/protocols/smithy-rpc-v2.html)
/// protocol.
pub struct RpcV2Cbor;
