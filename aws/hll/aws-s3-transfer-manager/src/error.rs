/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_types::byte_stream;
use std::io;

// TODO(design): revisit errors

/// Failed transfer result
#[derive(thiserror::Error, Debug)]
pub enum TransferError {
    /// The request was invalid
    #[error("invalid meta request: {0}")]
    InvalidMetaRequest(String),

    #[error("download failed")]
    DownloadFailed(#[from] DownloadError),
}

pub(crate) type GetObjectSdkError = ::aws_smithy_runtime_api::client::result::SdkError<
    aws_sdk_s3::operation::get_object::GetObjectError,
    ::aws_smithy_runtime_api::client::orchestrator::HttpResponse,
>;
pub(crate) type HeadObjectSdkError = ::aws_smithy_runtime_api::client::result::SdkError<
    aws_sdk_s3::operation::head_object::HeadObjectError,
    ::aws_smithy_runtime_api::client::orchestrator::HttpResponse,
>;

#[derive(thiserror::Error, Debug)]
pub enum DownloadError {
    #[error(transparent)]
    DiscoverFailed(SdkOperationError),

    #[error("download chunk failed")]
    ChunkFailed { source: SdkOperationError },
}

#[derive(thiserror::Error, Debug)]
pub enum SdkOperationError {
    #[error(transparent)]
    HeadObject(#[from] HeadObjectSdkError),

    #[error(transparent)]
    GetObject(#[from] GetObjectSdkError),

    #[error(transparent)]
    ReadError(#[from] byte_stream::error::Error),

    #[error(transparent)]
    IoError(#[from] io::Error),
}

// convenience to construct a TransferError from a chunk failure
pub(crate) fn chunk_failed<E: Into<SdkOperationError>>(e: E) -> TransferError {
    DownloadError::ChunkFailed { source: e.into() }.into()
}

pub(crate) fn invalid_meta_request(message: String) -> TransferError {
    TransferError::InvalidMetaRequest(message)
}
