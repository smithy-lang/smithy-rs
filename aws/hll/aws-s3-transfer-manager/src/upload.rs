/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::upload::request::UploadRequest;
use crate::upload::response::UploadResponse;

use crate::error::{TransferError, UploadError};
use crate::types::{ConcurrencySetting, TargetPartSize};
use crate::upload::handle::UploadHandle;
use crate::{DEFAULT_CONCURRENCY, MEBIBYTE};
use aws_types::SdkConfig;
use std::cmp;
use tokio::task::JoinSet;

mod handle;

/// Request types for uploads to Amazon S3
pub mod request;

/// Response types for uploads to Amazon S3
pub mod response;

/// Minimum upload part size in bytes
const MIN_PART_SIZE_BYTES: u64 = 5 * MEBIBYTE;

/// Maximum number of parts that a single S3 multipart upload supports
const MAX_PARTS: u64 = 10_000;

/// Fluent style builder for [Uploader]
#[derive(Debug, Clone)]
pub struct Builder {
    multipart_threshold_part_size: TargetPartSize,
    concurrency: ConcurrencySetting,
    sdk_config: Option<SdkConfig>,
}

impl Builder {
    fn new() -> Self {
        Self {
            multipart_threshold_part_size: TargetPartSize::Auto,
            concurrency: ConcurrencySetting::Auto,
            sdk_config: None,
        }
    }

    /// Minimum object size that should trigger a multipart upload.
    ///
    /// The minimum part size is 5 MiB, any part size less than that will be rounded up.
    /// Default is [TargetPartSize::Auto]
    pub fn multipart_threshold_part_size(
        mut self,
        multipart_threshold_part_size: TargetPartSize,
    ) -> Self {
        let threshold = match multipart_threshold_part_size {
            TargetPartSize::Explicit(part_size) => {
                TargetPartSize::Explicit(cmp::max(part_size, MIN_PART_SIZE_BYTES))
            }
            tps => tps,
        };
        self.multipart_threshold_part_size = threshold;
        self
    }

    /// Set the configuration used by the S3 client
    pub fn sdk_config(mut self, config: SdkConfig) -> Self {
        self.sdk_config = Some(config);
        self
    }

    /// Set the concurrency level this component is allowed to use.
    ///
    /// This sets the maximum number of concurrent in-flight requests.
    /// Default is [ConcurrencySetting::Auto].
    pub fn concurrency(mut self, concurrency: ConcurrencySetting) -> Self {
        self.concurrency = concurrency;
        self
    }

    /// Consumes the builder and constructs a [Uploader]
    pub fn build(self) -> Uploader {
        self.into()
    }
}

impl From<Builder> for Uploader {
    fn from(value: Builder) -> Self {
        let sdk_config = value
            .sdk_config
            .unwrap_or_else(|| SdkConfig::builder().build());
        let client = aws_sdk_s3::Client::new(&sdk_config);
        Self {
            multipart_threshold_part_size: value.multipart_threshold_part_size,
            concurrency: value.concurrency,
            client,
        }
    }
}

/// Upload an object in the most efficient way possible by splitting the request into
/// concurrent requests (e.g. using multi-part uploads).
#[derive(Debug, Clone)]
pub struct Uploader {
    multipart_threshold_part_size: TargetPartSize,
    concurrency: ConcurrencySetting,
    client: aws_sdk_s3::client::Client,
}

impl Uploader {
    /// Create a new [Builder]
    pub fn builder() -> Builder {
        Builder::new()
    }

    /// Upload a single object to Amazon S3.
    ///
    /// A single logical request may be split into many concurrent `UploadPart` requests
    /// to improve throughput.
    pub async fn upload(&self, req: UploadRequest) -> Result<UploadHandle, UploadError> {
        let size_hint = req.body().size_hint();
        let min_mpu_threshold = self.mpu_threshold_bytes();
        let mut tasks = JoinSet::new();

        if size_hint.lower() < min_mpu_threshold {
            tracing::trace!("upload request content size hint ({size_hint:?}) less than min part size threshold ({min_mpu_threshold}); sending as single PutObject request");
            // let resp = req.input.send_with(&self.client)?;
            todo!("send request as is")
        } else {
            // MPU has max of 10K parts which requires us to know the upper bound on the content (today anyway)
            let upper_bound = size_hint
                .upper()
                .ok_or_else(crate::io::error::Error::upper_bound_size_hint_required)?;
            let part_size = cmp::max(min_mpu_threshold, upper_bound / MAX_PARTS);
            tracing::trace!("upload request using multipart upload with part size: {part_size}");
            let mpu = start_mpu(&self.client, &req).await?;
            tracing::trace!(
                "multipart upload started with upload id: {:?}",
                mpu.upload_id
            );

            // FIXME - leftoff here, need parallel reading of the body which means likely moving
            // away from SdkBody for the public type to TM
            // let body = req.input.get_body().take().expect("body must be present");
            // let part_cnt = size_hint.lower() / part_size;
            // for i in part_cnt {
            //     // tasks.spawn(async {})
            // }
        }

        let handle = UploadHandle { _tasks: tasks };

        Ok(handle)
    }
    /// Get the concrete minimum upload size in bytes to use to determine whether multipart uploads
    /// are enabled for a given request.
    fn mpu_threshold_bytes(&self) -> u64 {
        match self.multipart_threshold_part_size {
            // FIXME(aws-sdk-rust#1159): add logic for determining this
            TargetPartSize::Auto => 8 * MEBIBYTE,
            TargetPartSize::Explicit(explicit) => explicit,
        }
    }
}

async fn start_mpu(
    client: &aws_sdk_s3::client::Client,
    req: &UploadRequest,
) -> Result<UploadResponse, UploadError> {
    let resp = client
        .create_multipart_upload()
        .set_acl(req.acl.clone())
        .set_bucket(req.bucket.clone())
        .set_cache_control(req.cache_control.clone())
        .set_content_disposition(req.content_disposition.clone())
        .set_content_encoding(req.content_encoding.clone())
        .set_content_language(req.content_language.clone())
        .set_content_type(req.content_type.clone())
        .set_expires(req.expires.clone())
        .set_grant_full_control(req.grant_full_control.clone())
        .set_grant_read(req.grant_read.clone())
        .set_grant_read_acp(req.grant_read_acp.clone())
        .set_grant_write_acp(req.grant_write_acp.clone())
        .set_key(req.key.clone())
        .set_metadata(req.metadata.clone())
        .set_server_side_encryption(req.server_side_encryption.clone())
        .set_storage_class(req.storage_class.clone())
        .set_website_redirect_location(req.website_redirect_location.clone())
        .set_sse_customer_algorithm(req.sse_customer_algorithm.clone())
        .set_sse_customer_key(req.sse_customer_key.clone())
        .set_sse_customer_key_md5(req.sse_customer_key_md5.clone())
        .set_ssekms_key_id(req.sse_kms_key_id.clone())
        .set_ssekms_encryption_context(req.sse_kms_encryption_context.clone())
        .set_bucket_key_enabled(req.bucket_key_enabled.clone())
        .set_request_payer(req.request_payer.clone())
        .set_tagging(req.tagging.clone())
        .set_object_lock_mode(req.object_lock_mode.clone())
        .set_object_lock_retain_until_date(req.object_lock_retain_until_date.clone())
        .set_object_lock_legal_hold_status(req.object_lock_legal_hold_status.clone())
        .set_expected_bucket_owner(req.expected_bucket_owner.clone())
        .set_checksum_algorithm(req.checksum_algorithm.clone())
        .send()
        .await?;

    unimplemented!()
    // Ok(resp.into())
}

// async fn upload_part(
//     ctx: UploadContext,
//     req: UploadRequest,
// )
