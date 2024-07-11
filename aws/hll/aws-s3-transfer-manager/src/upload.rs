/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

pub use crate::upload::request::UploadRequest;
pub use crate::upload::response::UploadResponse;

use crate::error::UploadError;
use crate::io::part_reader::{PartReaderBuilder, ReadPart};
use crate::io::InputStream;
use crate::types::{ConcurrencySetting, TargetPartSize};
use crate::upload::context::UploadContext;
use crate::upload::handle::UploadHandle;
use crate::{DEFAULT_CONCURRENCY, MEBIBYTE};
use aws_sdk_s3::types::CompletedPart;
use aws_smithy_types::byte_stream::ByteStream;
use aws_types::SdkConfig;
use bytes::Buf;
use std::cmp;
use std::sync::Arc;
use tracing::Instrument;

mod handle;

/// Request types for uploads to Amazon S3
pub mod request;

mod context;
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
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::error::Error;
    /// use std::path::Path;
    /// use aws_s3_transfer_manager::io::InputStream;
    /// use aws_s3_transfer_manager::upload::{Uploader, UploadRequest};
    ///
    /// async fn upload_file(client: Uploader, path: impl AsRef<Path>) -> Result<(), Box<dyn Error>> {
    ///     let stream = InputStream::from_path(path)?;
    ///     let request = UploadRequest::builder()
    ///         .bucket("my-bucket")
    ///         .key("my-key")
    ///         .body(stream)
    ///         .build()?;
    ///
    ///     let handle = client.upload(request).await?;
    ///
    ///     // upload() will return potentially before the request is complete.
    ///     // The handle given must be joined to drive the request to completion.
    ///     // It can also be used to get progress, pause or cancel the request, etc.
    ///     let response = handle.join().await?;
    ///     // ... do something with response
    ///     Ok(())
    /// }
    /// ```
    pub async fn upload(&self, mut req: UploadRequest) -> Result<UploadHandle, UploadError> {
        let min_mpu_threshold = self.mpu_threshold_bytes();

        // separate the body
        let stream = req.take_body();
        let ctx = self.new_context(req);
        let mut handle = UploadHandle::new(ctx);

        // MPU has max of 10K parts which requires us to know the upper bound on the content length (today anyway)
        let content_length = stream
            .size_hint()
            .upper()
            .ok_or_else(crate::io::error::Error::upper_bound_size_hint_required)?;

        if content_length < min_mpu_threshold {
            tracing::trace!("upload request content size hint ({content_length}) less than min part size threshold ({min_mpu_threshold}); sending as single PutObject request");
            // let resp = req.input.send_with(&self.client)?;
            todo!("adapt body to ByteStream and send request as is for non mpu upload")
        } else {
            self.try_start_mpu_upload(&mut handle, stream, content_length)
                .await?
        }

        Ok(handle)
    }

    fn new_context(&self, req: UploadRequest) -> UploadContext {
        UploadContext {
            client: self.client.clone(),
            request: Arc::new(req),
            upload_id: None,
        }
    }

    /// Upload the object via multipart upload
    ///
    /// # Arguments
    ///
    /// * `handle` - The upload handle
    /// * `stream` - The content to upload
    /// * `content_length` - The upper bound on the content length
    async fn try_start_mpu_upload(
        &self,
        handle: &mut UploadHandle,
        stream: InputStream,
        content_length: u64,
    ) -> Result<(), UploadError> {
        let part_size = cmp::max(self.mpu_threshold_bytes(), content_length / MAX_PARTS);
        tracing::trace!("upload request using multipart upload with part size: {part_size}");

        let mpu = start_mpu(&handle).await?;
        tracing::trace!(
            "multipart upload started with upload id: {:?}",
            mpu.upload_id
        );

        handle
            .ctx
            .set_upload_id(mpu.upload_id().expect("upload ID present").to_owned());

        let part_reader = Arc::new(
            PartReaderBuilder::new()
                .stream(stream)
                .part_size(part_size as usize)
                .build(),
        );

        let n_workers = self.num_workers();
        for i in 0..n_workers {
            let worker = upload_parts(handle.ctx.clone(), part_reader.clone())
                .instrument(tracing::debug_span!("upload-part", worker = i));
            handle.tasks.spawn(worker);
        }

        Ok(())
    }

    /// Get the concrete number of workers to use based on the concurrency setting.
    fn num_workers(&self) -> usize {
        match self.concurrency {
            // TODO(aws-sdk-rust#1159): add logic for determining this
            ConcurrencySetting::Auto => DEFAULT_CONCURRENCY,
            ConcurrencySetting::Explicit(explicit) => explicit,
        }
    }

    /// Get the concrete minimum upload size in bytes to use to determine whether multipart uploads
    /// are enabled for a given request.
    fn mpu_threshold_bytes(&self) -> u64 {
        match self.multipart_threshold_part_size {
            // TODO(aws-sdk-rust#1159): add logic for determining this
            TargetPartSize::Auto => 8 * MEBIBYTE,
            TargetPartSize::Explicit(explicit) => explicit,
        }
    }
}

/// start a new multipart upload by invoking `CreateMultipartUpload`
async fn start_mpu(handle: &UploadHandle) -> Result<UploadResponse, UploadError> {
    let req = handle.ctx.request();
    let client = handle.ctx.client();

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

    Ok(resp.into())
}

/// Worker function that pulls part data off the `reader` and uploads each part until the reader
/// is exhausted. If any part fails the worker will return the error and stop processing. If
/// the worker finishes successfully the completed parts uploaded by this worker are returned.
async fn upload_parts(
    ctx: UploadContext,
    reader: Arc<impl ReadPart>,
) -> Result<Vec<CompletedPart>, UploadError> {
    let mut completed_parts = Vec::new();
    loop {
        let part_result = reader.next_part().await?;
        let part_data = match part_result {
            Some(part_data) => part_data,
            None => break,
        };

        let part_number = part_data.part_number as i32;
        tracing::trace!("worker recv'd part number {}", part_number);

        let content_length = part_data.data.remaining();
        let body = ByteStream::from(part_data.data);

        // TODO(aws-sdk-rust#1159): disable payload signing
        // TODO(aws-sdk-rust#1159): set checksum fields if applicable
        let resp = ctx
            .client
            .upload_part()
            .set_bucket(ctx.request.bucket.clone())
            .set_key(ctx.request.key.clone())
            .set_upload_id(ctx.upload_id.clone())
            .part_number(part_number)
            .content_length(content_length as i64)
            .body(body)
            .set_sse_customer_algorithm(ctx.request.sse_customer_algorithm.clone())
            .set_sse_customer_key(ctx.request.sse_customer_key.clone())
            .set_sse_customer_key_md5(ctx.request.sse_customer_key_md5.clone())
            .set_request_payer(ctx.request.request_payer.clone())
            .set_expected_bucket_owner(ctx.request.expected_bucket_owner.clone())
            .send()
            .await?;

        let completed = CompletedPart::builder()
            .part_number(part_number)
            .set_e_tag(resp.e_tag.clone())
            .set_checksum_crc32(resp.checksum_crc32.clone())
            .set_checksum_crc32_c(resp.checksum_crc32_c.clone())
            .set_checksum_sha1(resp.checksum_sha1.clone())
            .set_checksum_sha256(resp.checksum_sha256.clone())
            .build();

        completed_parts.push(completed);
    }

    tracing::trace!("no more parts, worker finished");
    Ok(completed_parts)
}

#[cfg(test)]
mod test {}
