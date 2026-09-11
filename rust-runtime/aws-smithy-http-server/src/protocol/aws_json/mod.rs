/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

pub mod rejection;
pub mod router;
pub mod runtime_error;

pub(crate) fn schema_codec() -> aws_smithy_json::codec::JsonCodec {
    aws_smithy_json::codec::JsonCodec::new(
        aws_smithy_json::codec::JsonCodecSettings::builder()
            .use_json_name(false)
            .default_timestamp_format(aws_smithy_types::date_time::Format::EpochSeconds)
            .strict_timestamp_formats(true)
            .reject_unknown_union_members(true)
            .build(),
    )
}
