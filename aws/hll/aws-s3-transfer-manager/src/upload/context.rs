/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::upload::UploadRequest;
use std::ops::Deref;
use std::sync::Arc;

/// Internal context used to drive a single Upload request
#[derive(Debug, Clone)]
pub(crate) struct UploadContext {
    /// client used for SDK operations
    pub(crate) client: aws_sdk_s3::Client,
    /// the multipart upload ID
    pub(crate) upload_id: Option<String>,
    /// the original request (NOTE: the body will have been taken for processing, only the other fields remain)
    pub(crate) request: Arc<UploadRequest>,
}

impl UploadContext {
    /// The S3 client to use for SDK operations
    pub(crate) fn client(&self) -> &aws_sdk_s3::Client {
        &self.client
    }

    /// The original request (sans the body as it will have been taken for processing)
    pub(crate) fn request(&self) -> &UploadRequest {
        self.request.deref()
    }

    /// Set the upload ID if the transfer will be done using a multipart upload
    pub(crate) fn set_upload_id(&mut self, upload_id: String) {
        self.upload_id = Some(upload_id)
    }

    /// Check if this transfer is using multipart upload
    pub(crate) fn is_multipart_upload(&self) -> bool {
        self.upload_id.is_some()
    }
}
