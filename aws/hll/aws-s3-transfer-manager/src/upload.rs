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
use crate::upload::response::UploadResponseBuilder;
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
    client: Option<aws_sdk_s3::Client>,
}

impl Builder {
    fn new() -> Self {
        Self {
            multipart_threshold_part_size: TargetPartSize::Auto,
            concurrency: ConcurrencySetting::Auto,
            sdk_config: None,
            client: None,
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

        self.set_multipart_threshold_part_size(threshold)
    }

    /// Minimum object size that should trigger a multipart upload.
    ///
    /// NOTE: This does not validate the setting and is meant for internal use only.
    pub(crate) fn set_multipart_threshold_part_size(mut self, threshold: TargetPartSize) -> Self {
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

    /// Set an explicit client to use. This takes precedence over setting `sdk_config`.
    pub(crate) fn client(mut self, client: aws_sdk_s3::Client) -> Self {
        self.client = Some(client);
        self
    }
}

impl From<Builder> for Uploader {
    fn from(value: Builder) -> Self {
        let client = value.client.unwrap_or_else(|| {
            let sdk_config = value
                .sdk_config
                .unwrap_or_else(|| SdkConfig::builder().build());
            aws_sdk_s3::Client::new(&sdk_config)
        });

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

        handle.set_response(mpu);

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
async fn start_mpu(handle: &UploadHandle) -> Result<UploadResponseBuilder, UploadError> {
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
mod test {
    use crate::io::InputStream;
    use crate::types::{ConcurrencySetting, TargetPartSize};
    use crate::upload::{Builder, UploadRequest, Uploader};
    use aws_sdk_s3::operation::complete_multipart_upload::CompleteMultipartUploadOutput;
    use aws_sdk_s3::operation::create_multipart_upload::CreateMultipartUploadOutput;
    use aws_sdk_s3::operation::upload_part::UploadPartOutput;
    use aws_smithy_mocks_experimental::{mock, mock_client, RuleMode};
    use bytes::Bytes;
    use std::ops::Deref;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_basic_mpu() {
        let expected_upload_id = Arc::new("test-upload".to_owned());
        let body = Bytes::from_static(b"every adolescent dog goes bonkers early");
        let stream = InputStream::from(body);

        let upload_id = expected_upload_id.clone();
        let create_mpu =
            mock!(aws_sdk_s3::Client::create_multipart_upload).then_output(move || {
                CreateMultipartUploadOutput::builder()
                    .upload_id(upload_id.as_ref().to_owned())
                    .build()
            });

        let upload_id = expected_upload_id.clone();
        let upload_1 = mock!(aws_sdk_s3::Client::upload_part)
            .match_requests(move |r| {
                r.upload_id.as_ref() == Some(&upload_id) && r.content_length == Some(30)
            })
            .then_output(|| UploadPartOutput::builder().build());

        let upload_id = expected_upload_id.clone();
        let upload_2 = mock!(aws_sdk_s3::Client::upload_part)
            .match_requests(move |r| {
                r.upload_id.as_ref() == Some(&upload_id) && r.content_length == Some(9)
            })
            .then_output(|| UploadPartOutput::builder().build());

        let expected_e_tag = Arc::new("test-e-tag".to_owned());
        let upload_id = expected_upload_id.clone();
        let e_tag = expected_e_tag.clone();
        let complete_mpu = mock!(aws_sdk_s3::Client::complete_multipart_upload)
            .match_requests(move |r| {
                r.upload_id.as_ref() == Some(&upload_id)
                    && r.multipart_upload.clone().unwrap().parts.unwrap().len() == 2
            })
            .then_output(move || {
                CompleteMultipartUploadOutput::builder()
                    .e_tag(e_tag.as_ref().to_owned())
                    .build()
            });

        let client = mock_client!(
            aws_sdk_s3,
            RuleMode::Sequential,
            &[&create_mpu, &upload_1, &upload_2, &complete_mpu]
        );

        let uploader = Uploader::builder()
            .concurrency(ConcurrencySetting::Explicit(1))
            .set_multipart_threshold_part_size(TargetPartSize::Explicit(30))
            .client(client)
            .build();

        let request = UploadRequest::builder()
            .bucket("test-bucket")
            .key("test-key")
            .body(stream)
            .build()
            .unwrap();

        let handle = uploader.upload(request).await.unwrap();

        let resp = handle.join().await.unwrap();
        assert_eq!(expected_upload_id.deref(), resp.upload_id.unwrap().deref());
        assert_eq!(expected_e_tag.deref(), resp.e_tag.unwrap().deref());
    }
}
