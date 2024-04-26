/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/// Shared handle used across a single download request
#[derive(Debug, Clone)]
pub(super) struct DownloadHandle {
    pub(crate) client: aws_sdk_s3::Client,
    pub(crate) target_part_size: u64,
}
