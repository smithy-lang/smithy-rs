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

    /// A download failed
    #[error("download failed")]
    DownloadFailed(#[from] DownloadError),

    /// A upload failed
    #[error("upload failed")]
    UploadFailed(#[from] UploadError),
}

pub(crate) type GetObjectSdkError = ::aws_smithy_runtime_api::client::result::SdkError<
    aws_sdk_s3::operation::get_object::GetObjectError,
    ::aws_smithy_runtime_api::client::orchestrator::HttpResponse,
>;
pub(crate) type HeadObjectSdkError = ::aws_smithy_runtime_api::client::result::SdkError<
    aws_sdk_s3::operation::head_object::HeadObjectError,
    ::aws_smithy_runtime_api::client::orchestrator::HttpResponse,
>;

/// An error related to downloading an object
#[derive(thiserror::Error, Debug)]
pub enum DownloadError {
    /// Discovery of object metadata failed
    #[error(transparent)]
    DiscoverFailed(SdkOperationError),

    /// A failure occurred fetching a single chunk of the overall object data
    #[error("download chunk failed")]
    ChunkFailed {
        /// The underlying SDK error
        source: SdkOperationError,
    },
}

pub(crate) type CreateMultipartUploadSdkError = ::aws_smithy_runtime_api::client::result::SdkError<
    aws_sdk_s3::operation::create_multipart_upload::CreateMultipartUploadError,
    ::aws_smithy_runtime_api::client::orchestrator::HttpResponse,
>;

pub(crate) type UploadPartSdkError = ::aws_smithy_runtime_api::client::result::SdkError<
    aws_sdk_s3::operation::upload_part::UploadPartError,
    ::aws_smithy_runtime_api::client::orchestrator::HttpResponse,
>;

/// An error related to upload an object
#[derive(thiserror::Error, Debug)]
pub enum UploadError {
    /// An error occurred invoking [aws_sdk_s3::Client::CreateMultipartUpload]
    #[error(transparent)]
    CreateMultipartUpload(#[from] CreateMultipartUploadSdkError),

    /// An error occurred invoking [aws_sdk_s3::Client::UploadPart]
    #[error(transparent)]
    UploadPart(#[from] UploadPartSdkError),
}

/// An underlying S3 SDK error
#[derive(thiserror::Error, Debug)]
pub enum SdkOperationError {
    /// An error occurred invoking [aws_sdk_s3::Client::head_object]
    #[error(transparent)]
    HeadObject(#[from] HeadObjectSdkError),

    /// An error occurred invoking [aws_sdk_s3::Client::get_object]
    #[error(transparent)]
    GetObject(#[from] GetObjectSdkError),

    /// An error occurred reading the underlying data
    #[error(transparent)]
    ReadError(#[from] byte_stream::error::Error),

    /// An unknown IO error occurred carrying out the request
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

impl From<CreateMultipartUploadSdkError> for TransferError {
    fn from(value: CreateMultipartUploadSdkError) -> Self {
        TransferError::UploadFailed(value.into())
    }
}
