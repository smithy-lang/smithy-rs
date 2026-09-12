/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

pub mod router;

/// [AWS JSON 1.0](https://smithy.io/2.0/aws/protocols/aws-json-1_0-protocol.html) protocol.
#[derive(Debug, Default, Clone, Copy)]
pub struct AwsJson1_0;

/// Stateful schema-driven AWS JSON 1.0 protocol implementation.
#[derive(Debug)]
pub struct AwsJson1_0Protocol {
    pub(crate) inner: crate::schema::protocol::rpc::RpcProtocol<aws_smithy_json::codec::JsonCodec>,
}

impl Default for AwsJson1_0Protocol {
    fn default() -> Self {
        Self {
            inner: crate::schema::protocol::rpc::RpcProtocol::new(
                super::aws_json::schema_codec(),
                "application/x-amz-json-1.0",
                Some("application/x-amz-json-1.0"),
                crate::schema::protocol::rpc::RpcAccept::Always,
                crate::schema::protocol::rpc::RpcStreaming::CodecContentType,
            ),
        }
    }
}
