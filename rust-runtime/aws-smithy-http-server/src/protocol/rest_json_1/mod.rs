/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

pub mod rejection;
pub mod router;
pub mod runtime_error;

/// [AWS restJson1](https://smithy.io/2.0/aws/protocols/aws-restjson1-protocol.html) protocol.
#[derive(Debug, Default, Clone, Copy)]
pub struct RestJson1;

/// Stateful schema-driven restJson1 protocol implementation.
#[derive(Debug)]
pub struct RestJson1Protocol {
    pub(crate) inner: crate::schema::protocol::rest::RestProtocol<aws_smithy_json::codec::JsonCodec>,
}

impl Default for RestJson1Protocol {
    fn default() -> Self {
        Self {
            inner: crate::schema::protocol::rest::RestProtocol::new(
                aws_smithy_json::codec::JsonCodec::new(
                    aws_smithy_json::codec::JsonCodecSettings::builder()
                        .use_json_name(true)
                        .default_timestamp_format(aws_smithy_types::date_time::Format::EpochSeconds)
                        .strict_timestamp_formats(true)
                        .reject_unknown_union_members(true)
                        .build(),
                ),
                "application/json",
            ),
        }
    }
}
