/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/// Shared context used across a single download request
#[derive(Debug, Clone)]
pub(crate) struct DownloadContext {
    pub(crate) client: aws_sdk_s3::Client,
    pub(crate) target_part_size: u64,
}
