/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use crate::download::body::Body;
use crate::download::object_meta::ObjectMetadata;
use tokio::task;

/// Response type for a single download object request.
#[derive(Debug)]
#[non_exhaustive]
pub struct DownloadHandle {
    /// Object metadata
    pub object_meta: ObjectMetadata,

    /// The object content
    pub body: Body,

    /// All child tasks spawned for this download
    pub(crate) tasks: task::JoinSet<()>,
}

impl DownloadHandle {
    /// Object metadata
    pub fn object_meta(&self) -> &ObjectMetadata {
        &self.object_meta
    }

    /// Object content
    pub fn body(&self) -> &Body {
        &self.body
    }
}
