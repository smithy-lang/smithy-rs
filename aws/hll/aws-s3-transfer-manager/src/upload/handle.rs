/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::error::UploadError;
use crate::upload::context::UploadContext;
use crate::upload::UploadResponse;
use tokio::task;

/// Response type for a single upload object request.
#[derive(Debug)]
#[non_exhaustive]
pub struct UploadHandle {
    /// All child tasks spawned for this upload
    pub(crate) tasks: task::JoinSet<Result<(), UploadError>>,
    pub(crate) ctx: UploadContext,
}

impl UploadHandle {
    /// Create a new upload handle with the given request context
    pub(crate) fn new(ctx: UploadContext) -> Self {
        Self {
            tasks: task::JoinSet::new(),
            ctx,
        }
    }

    /// Consume the handle and wait for upload to complete
    pub async fn join(mut self) -> Result<UploadResponse, UploadError> {
        unimplemented!()
    }

    /// Abort the upload and cancel any in-progress part uploads.
    pub async fn abort(&mut self) {
        // cancel in-progress uploads
        self.tasks.abort_all();
        // join all tasks
        while let Some(_) = self.tasks.join_next().await {}

        // TODO - invoke abort multipart upload depending on the policy
        unimplemented!()
    }

    /// Pause the upload and return a handle that can be used to resume the upload.
    pub fn pause(mut self) -> PausedUploadHandle {
        unimplemented!()
    }

    // pub fn progress() -> Progess
}

pub struct PausedUploadHandle {}
