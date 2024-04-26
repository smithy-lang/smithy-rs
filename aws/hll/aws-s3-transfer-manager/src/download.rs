/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

mod discovery;
mod handle;
mod header;
mod object_meta;

use aws_sdk_s3::operation::get_object::builders::{GetObjectFluentBuilder, GetObjectInputBuilder};

use self::object_meta::ObjectMetadata;

pub struct DownloadRequest {
    pub(crate) input: GetObjectInputBuilder,
}

// FIXME - should probably be TryFrom since checksums may conflict?
impl From<GetObjectFluentBuilder> for DownloadRequest {
    fn from(value: GetObjectFluentBuilder) -> Self {
        Self {
            input: value.as_input().clone(),
        }
    }
}

impl From<GetObjectInputBuilder> for DownloadRequest {
    fn from(value: GetObjectInputBuilder) -> Self {
        Self { input: value }
    }
}

// FIXME - expose common fields?
pub struct DownloadResponse {
    /// Object metadata
    pub object_meta: ObjectMetadata,
}
