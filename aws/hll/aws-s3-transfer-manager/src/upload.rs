mod handle;

use crate::error::TransferError;
use crate::types::{ConcurrencySetting, SizeHint, TargetPartSize};
use crate::upload::handle::{UploadHandle, UploadResponse};
use crate::{DEFAULT_CONCURRENCY, MEBIBYTE};
use aws_sdk_s3::operation::put_object::builders::PutObjectInputBuilder;
use aws_types::SdkConfig;
use std::cmp;
use tokio::task::JoinSet;

/// Minimum upload part size in bytes
const MIN_PART_SIZE_BYTES: u64 = 5 * MEBIBYTE;

/// Maximum number of parts that a single S3 multipart upload supports
const MAX_PARTS: u64 = 10_000;

/// Request type for uploading a single object
#[derive(Debug)]
#[non_exhaustive]
pub struct UploadRequest {
    pub(crate) input: PutObjectInputBuilder,
}

// FIXME(aws-sdk-rust#1159): PutObjectInputBuilder doesn't implement Clone because of ByteStream
// impl From<PutObjectFluentBuilder> for UploadRequest {
//     fn from(value: PutObjectFluentBuilder) -> Self {
//         let builder = PutObjectInputBuilder::default();
//         let body = value.get_body().take();
//
//         let builder = builder
//             .set_body(body)
//             .set_acl(value.get_acl().to_owned())
//             .set_bucket(value.get_bucket().to_owned())
//             .set_cache_control(value.get_cache_control().to_owned())
//             .set_content_disposition(value.get_content_disposition().to_owned())
//             .set_content_encoding(value.get_content_encoding().to_owned())
//             .set_content_language(value.get_content_language().to_owned())
//             .set_content_length(value.get_content_length().to_owned())
//             .set_content_md5(value.get_content_md5().to_owned())
//             .set_content_type(value.get_content_type().to_owned())
//             .set_checksum_algorithm(value.get_checksum_algorithm().to_owned())
//             .set_checksum_crc32(value.get_checksum_crc32().to_owned())
//             .set_checksum_crc32_c(value.get_checksum_crc32_c().to_owned())
//             .set_checksum_sha1(value.get_checksum_sha1().to_owned())
//             .set_checksum_sha256(value.get_checksum_sha256().to_owned())
//             .set_expires(value.get_expires().to_owned())
//             .set_grant_full_control(value.get_grant_full_control().to_owned())
//             .set_grant_read(value.get_grant_read().to_owned())
//             .set_grant_read_acp(value.get_grant_read_acp().to_owned())
//             .set_grant_write_acp(value.get_grant_write_acp().to_owned())
//             .set_key(value.get_key().to_owned())
//             .set_metadata(value.get_metadata().to_owned())
//             .set_server_side_encryption(value.get_server_side_encryption().to_owned())
//             .set_storage_class(value.get_storage_class().to_owned())
//             .set_website_redirect_location(value.get_website_redirect_location().to_owned())
//             .set_sse_customer_algorithm(value.get_sse_customer_algorithm().to_owned())
//             .set_sse_customer_key(value.get_sse_customer_key().to_owned())
//             .set_sse_customer_key_md5(value.get_sse_customer_key_md5().to_owned())
//             .set_ssekms_key_id(value.get_ssekms_key_id().to_owned())
//             .set_ssekms_encryption_context(value.get_ssekms_encryption_context().to_owned())
//             .set_bucket_key_enabled(value.get_bucket_key_enabled().to_owned())
//             .set_request_payer(value.get_request_payer().to_owned())
//             .set_tagging(value.get_tagging().to_owned())
//             .set_object_lock_mode(value.get_object_lock_mode().to_owned())
//             .set_object_lock_retain_until_date(value.get_object_lock_retain_until_date().to_owned())
//             .set_object_lock_legal_hold_status(value.get_object_lock_legal_hold_status().to_owned())
//             .set_expected_bucket_owner(value.get_expected_bucket_owner().to_owned());
//
//         Self { input: builder }
//     }
// }

impl From<PutObjectInputBuilder> for UploadRequest {
    fn from(value: PutObjectInputBuilder) -> Self {
        Self { input: value }
    }
}

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
            multipart_threshold_part_size: TargetPartSize::Explicit(8 * MEBIBYTE),
            concurrency: ConcurrencySetting::Explicit(DEFAULT_CONCURRENCY),
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

impl UploadRequest {
    fn size_hint(&self) -> Result<SizeHint, TransferError> {
        let content_length = self.input.get_content_length();
        let body_size_hint = self.input.get_body().as_ref().map(|b| b.size_hint());
        match (body_size_hint, content_length) {
            (Some(hint), _) => Ok(SizeHint::default().with_lower(hint.0).with_upper(hint.1)),
            (None, Some(content_len)) => Ok(SizeHint::exact(*content_len as u64)),
            (None, None) => Err(TransferError::InvalidMetaRequest(
                "upload content length must be known".to_string(),
            )),
        }
    }
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
    pub async fn upload(&self, req: UploadRequest) -> Result<UploadHandle, TransferError> {
        let size_hint = req.size_hint()?;
        let min_mpu_threshold = self.mpu_threshold_bytes();
        let mut tasks = JoinSet::new();

        if size_hint.lower() < min_mpu_threshold {
            tracing::trace!("upload request content size hint ({size_hint:?}) less than min part size threshold ({min_mpu_threshold}); sending as single PutObject request");
            // let resp = req.input.send_with(&self.client)?;
            todo!("send request as is")
        } else {
            let part_size = cmp::max(min_mpu_threshold, size_hint.lower() / MAX_PARTS);
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
            TargetPartSize::Auto => MIN_PART_SIZE_BYTES,
            TargetPartSize::Explicit(explicit) => explicit,
        }
    }
}

async fn start_mpu(
    client: &aws_sdk_s3::client::Client,
    req: &UploadRequest,
) -> Result<UploadResponse, TransferError> {
    let resp = client
        .create_multipart_upload()
        .set_acl(req.input.get_acl().clone())
        .set_bucket(req.input.get_bucket().clone())
        .set_cache_control(req.input.get_cache_control().clone())
        .set_content_disposition(req.input.get_content_disposition().clone())
        .set_content_encoding(req.input.get_content_encoding().clone())
        .set_content_language(req.input.get_content_language().clone())
        .set_content_type(req.input.get_content_type().clone())
        .set_expires(req.input.get_expires().clone())
        .set_grant_full_control(req.input.get_grant_full_control().clone())
        .set_grant_read(req.input.get_grant_read().clone())
        .set_grant_read_acp(req.input.get_grant_read_acp().clone())
        .set_grant_write_acp(req.input.get_grant_write_acp().clone())
        .set_key(req.input.get_key().clone())
        .set_metadata(req.input.get_metadata().clone())
        .set_server_side_encryption(req.input.get_server_side_encryption().clone())
        .set_storage_class(req.input.get_storage_class().clone())
        .set_website_redirect_location(req.input.get_website_redirect_location().clone())
        .set_sse_customer_algorithm(req.input.get_sse_customer_algorithm().clone())
        .set_sse_customer_key(req.input.get_sse_customer_key().clone())
        .set_sse_customer_key_md5(req.input.get_sse_customer_key_md5().clone())
        .set_ssekms_key_id(req.input.get_ssekms_key_id().clone())
        .set_ssekms_encryption_context(req.input.get_ssekms_encryption_context().clone())
        .set_bucket_key_enabled(req.input.get_bucket_key_enabled().clone())
        .set_request_payer(req.input.get_request_payer().clone())
        .set_tagging(req.input.get_tagging().clone())
        .set_object_lock_mode(req.input.get_object_lock_mode().clone())
        .set_object_lock_retain_until_date(req.input.get_object_lock_retain_until_date().clone())
        .set_object_lock_legal_hold_status(req.input.get_object_lock_legal_hold_status().clone())
        .set_expected_bucket_owner(req.input.get_expected_bucket_owner().clone())
        .set_checksum_algorithm(req.input.get_checksum_algorithm().clone())
        .send()
        .await?;

    unimplemented!()
    // Ok(resp.into())
}

// async fn upload_part(
//     ctx: UploadContext,
//     req: UploadRequest,
// )
