/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_s3::operation::create_multipart_upload::CreateMultipartUploadOutput;
use std::fmt::{Debug, Formatter};

/// Common response fields for uploading an object to Amazon S3
#[non_exhaustive]
#[derive(Clone, PartialEq)]
pub struct UploadResponse {
    /// <p>If the expiration is configured for the object (see <a href="https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutBucketLifecycleConfiguration.html">PutBucketLifecycleConfiguration</a>) in the <i>Amazon S3 User Guide</i>, the response includes this header. It includes the <code>expiry-date</code> and <code>rule-id</code> key-value pairs that provide information about object expiration. The value of the <code>rule-id</code> is URL-encoded.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub expiration: Option<String>,

    /// <p>Entity tag for the uploaded object.</p>
    /// <p><b>General purpose buckets </b> - To ensure that data is not corrupted traversing the network, for objects where the ETag is the MD5 digest of the object, you can calculate the MD5 while putting an object to Amazon S3 and compare the returned ETag to the calculated MD5 value.</p>
    /// <p><b>Directory buckets </b> - The ETag for the object in a directory bucket isn't the MD5 digest of the object.</p>
    pub e_tag: Option<String>,

    /// <p>The base64-encoded, 32-bit CRC32 checksum of the object. This will only be present if it was uploaded with the object. When you use an API operation on an object that was uploaded using multipart uploads, this value may not be a direct checksum value of the full object. Instead, it's a calculation based on the checksum values of each individual part. For more information about how checksums are calculated with multipart uploads, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html#large-object-checksums"> Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub checksum_crc32: Option<String>,

    /// <p>The base64-encoded, 32-bit CRC32C checksum of the object. This will only be present if it was uploaded with the object. When you use an API operation on an object that was uploaded using multipart uploads, this value may not be a direct checksum value of the full object. Instead, it's a calculation based on the checksum values of each individual part. For more information about how checksums are calculated with multipart uploads, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html#large-object-checksums"> Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub checksum_crc32_c: Option<String>,

    /// <p>The base64-encoded, 160-bit SHA-1 digest of the object. This will only be present if it was uploaded with the object. When you use the API operation on an object that was uploaded using multipart uploads, this value may not be a direct checksum value of the full object. Instead, it's a calculation based on the checksum values of each individual part. For more information about how checksums are calculated with multipart uploads, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html#large-object-checksums"> Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub checksum_sha1: Option<String>,

    /// <p>The base64-encoded, 256-bit SHA-256 digest of the object. This will only be present if it was uploaded with the object. When you use an API operation on an object that was uploaded using multipart uploads, this value may not be a direct checksum value of the full object. Instead, it's a calculation based on the checksum values of each individual part. For more information about how checksums are calculated with multipart uploads, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html#large-object-checksums"> Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub checksum_sha256: Option<String>,

    /// <p>The server-side encryption algorithm used when you store this object in Amazon S3 (for example, <code>AES256</code>, <code>aws:kms</code>, <code>aws:kms:dsse</code>).</p><note>
    /// <p>For directory buckets, only server-side encryption with Amazon S3 managed keys (SSE-S3) (<code>AES256</code>) is supported.</p>
    /// </note>
    pub server_side_encryption: Option<aws_sdk_s3::types::ServerSideEncryption>,

    /// <p>Version ID of the object.</p>
    /// <p>If you enable versioning for a bucket, Amazon S3 automatically generates a unique version ID for the object being stored. Amazon S3 returns this ID in the response. When you enable versioning for a bucket, if Amazon S3 receives multiple write requests for the same object simultaneously, it stores all of the objects. For more information about versioning, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/AddingObjectstoVersioningEnabledBuckets.html">Adding Objects to Versioning-Enabled Buckets</a> in the <i>Amazon S3 User Guide</i>. For information about returning the versioning state of a bucket, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketVersioning.html">GetBucketVersioning</a>.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub version_id: Option<String>,

    /// <p>If server-side encryption with a customer-provided encryption key was requested, the response will include this header to confirm the encryption algorithm that's used.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub sse_customer_algorithm: Option<String>,

    /// <p>If server-side encryption with a customer-provided encryption key was requested, the response will include this header to provide the round-trip message integrity verification of the customer-provided encryption key.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub sse_customer_key_md5: Option<String>,

    /// <p>If <code>x-amz-server-side-encryption</code> has a valid value of <code>aws:kms</code> or <code>aws:kms:dsse</code>, this header indicates the ID of the Key Management Service (KMS) symmetric encryption customer managed key that was used for the object.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub sse_kms_key_id: Option<String>,

    /// <p>If present, indicates the Amazon Web Services KMS Encryption Context to use for object encryption. The value of this header is a base64-encoded UTF-8 string holding JSON with the encryption context key-value pairs. This value is stored as object metadata and automatically gets passed on to Amazon Web Services KMS for future <code>GetObject</code> or <code>CopyObject</code> operations on this object.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub sse_kms_encryption_context: Option<String>,

    /// <p>Indicates whether the uploaded object uses an S3 Bucket Key for server-side encryption with Key Management Service (KMS) keys (SSE-KMS).</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub bucket_key_enabled: Option<bool>,

    /// <p>If present, indicates that the requester was successfully charged for the request.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub request_charged: Option<aws_sdk_s3::types::RequestCharged>,

    /// <p>ID for the initiated multipart upload.</p>
    /// This will not be set for requests that are not split into multipart uploads.
    pub upload_id: Option<String>,
}

impl UploadResponse {
    /// <p>If the expiration is configured for the object (see <a href="https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutBucketLifecycleConfiguration.html">PutBucketLifecycleConfiguration</a>) in the <i>Amazon S3 User Guide</i>, the response includes this header. It includes the <code>expiry-date</code> and <code>rule-id</code> key-value pairs that provide information about object expiration. The value of the <code>rule-id</code> is URL-encoded.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn expiration(&self) -> Option<&str> {
        self.expiration.as_deref()
    }
    /// <p>Entity tag for the uploaded object.</p>
    /// <p><b>General purpose buckets </b> - To ensure that data is not corrupted traversing the network, for objects where the ETag is the MD5 digest of the object, you can calculate the MD5 while putting an object to Amazon S3 and compare the returned ETag to the calculated MD5 value.</p>
    /// <p><b>Directory buckets </b> - The ETag for the object in a directory bucket isn't the MD5 digest of the object.</p>
    pub fn e_tag(&self) -> Option<&str> {
        self.e_tag.as_deref()
    }
    /// <p>The base64-encoded, 32-bit CRC32 checksum of the object. This will only be present if it was uploaded with the object. When you use an API operation on an object that was uploaded using multipart uploads, this value may not be a direct checksum value of the full object. Instead, it's a calculation based on the checksum values of each individual part. For more information about how checksums are calculated with multipart uploads, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html#large-object-checksums"> Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn checksum_crc32(&self) -> Option<&str> {
        self.checksum_crc32.as_deref()
    }
    /// <p>The base64-encoded, 32-bit CRC32C checksum of the object. This will only be present if it was uploaded with the object. When you use an API operation on an object that was uploaded using multipart uploads, this value may not be a direct checksum value of the full object. Instead, it's a calculation based on the checksum values of each individual part. For more information about how checksums are calculated with multipart uploads, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html#large-object-checksums"> Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn checksum_crc32_c(&self) -> Option<&str> {
        self.checksum_crc32_c.as_deref()
    }
    /// <p>The base64-encoded, 160-bit SHA-1 digest of the object. This will only be present if it was uploaded with the object. When you use the API operation on an object that was uploaded using multipart uploads, this value may not be a direct checksum value of the full object. Instead, it's a calculation based on the checksum values of each individual part. For more information about how checksums are calculated with multipart uploads, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html#large-object-checksums"> Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn checksum_sha1(&self) -> Option<&str> {
        self.checksum_sha1.as_deref()
    }
    /// <p>The base64-encoded, 256-bit SHA-256 digest of the object. This will only be present if it was uploaded with the object. When you use an API operation on an object that was uploaded using multipart uploads, this value may not be a direct checksum value of the full object. Instead, it's a calculation based on the checksum values of each individual part. For more information about how checksums are calculated with multipart uploads, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html#large-object-checksums"> Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn checksum_sha256(&self) -> Option<&str> {
        self.checksum_sha256.as_deref()
    }
    /// <p>The server-side encryption algorithm used when you store this object in Amazon S3 (for example, <code>AES256</code>, <code>aws:kms</code>, <code>aws:kms:dsse</code>).</p><note>
    /// <p>For directory buckets, only server-side encryption with Amazon S3 managed keys (SSE-S3) (<code>AES256</code>) is supported.</p>
    /// </note>
    pub fn server_side_encryption(&self) -> Option<&aws_sdk_s3::types::ServerSideEncryption> {
        self.server_side_encryption.as_ref()
    }
    /// <p>Version ID of the object.</p>
    /// <p>If you enable versioning for a bucket, Amazon S3 automatically generates a unique version ID for the object being stored. Amazon S3 returns this ID in the response. When you enable versioning for a bucket, if Amazon S3 receives multiple write requests for the same object simultaneously, it stores all of the objects. For more information about versioning, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/AddingObjectstoVersioningEnabledBuckets.html">Adding Objects to Versioning-Enabled Buckets</a> in the <i>Amazon S3 User Guide</i>. For information about returning the versioning state of a bucket, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketVersioning.html">GetBucketVersioning</a>.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn version_id(&self) -> Option<&str> {
        self.version_id.as_deref()
    }
    /// <p>If server-side encryption with a customer-provided encryption key was requested, the response will include this header to confirm the encryption algorithm that's used.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn sse_customer_algorithm(&self) -> Option<&str> {
        self.sse_customer_algorithm.as_deref()
    }
    /// <p>If server-side encryption with a customer-provided encryption key was requested, the response will include this header to provide the round-trip message integrity verification of the customer-provided encryption key.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn sse_customer_key_md5(&self) -> Option<&str> {
        self.sse_customer_key_md5.as_deref()
    }
    /// <p>If <code>x-amz-server-side-encryption</code> has a valid value of <code>aws:kms</code> or <code>aws:kms:dsse</code>, this header indicates the ID of the Key Management Service (KMS) symmetric encryption customer managed key that was used for the object.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn sse_kms_key_id(&self) -> Option<&str> {
        self.sse_kms_key_id.as_deref()
    }
    /// <p>If present, indicates the Amazon Web Services KMS Encryption Context to use for object encryption. The value of this header is a base64-encoded UTF-8 string holding JSON with the encryption context key-value pairs. This value is stored as object metadata and automatically gets passed on to Amazon Web Services KMS for future <code>GetObject</code> or <code>CopyObject</code> operations on this object.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn sse_kms_encryption_context(&self) -> Option<&str> {
        self.sse_kms_encryption_context.as_deref()
    }
    /// <p>Indicates whether the uploaded object uses an S3 Bucket Key for server-side encryption with Key Management Service (KMS) keys (SSE-KMS).</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn bucket_key_enabled(&self) -> Option<bool> {
        self.bucket_key_enabled
    }
    /// <p>If present, indicates that the requester was successfully charged for the request.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn request_charged(&self) -> Option<&aws_sdk_s3::types::RequestCharged> {
        self.request_charged.as_ref()
    }

    /// <p>ID for the initiated multipart upload.</p>
    /// This will not be set for requests that are not split into multipart uploads.
    pub fn upload_id(&self) -> Option<&String> {
        self.upload_id.as_ref()
    }
}

impl Debug for UploadResponse {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut formatter = f.debug_struct("UploadResponse");
        formatter.field("expiration", &self.expiration);
        formatter.field("e_tag", &self.e_tag);
        formatter.field("checksum_crc32", &self.checksum_crc32);
        formatter.field("checksum_crc32_c", &self.checksum_crc32_c);
        formatter.field("checksum_sha1", &self.checksum_sha1);
        formatter.field("checksum_sha256", &self.checksum_sha256);
        formatter.field("server_side_encryption", &self.server_side_encryption);
        formatter.field("version_id", &self.version_id);
        formatter.field("sse_customer_algorithm", &self.sse_customer_algorithm);
        formatter.field("sse_customer_key_md5", &self.sse_customer_key_md5);
        formatter.field("sse_kms_key_id", &"*** Sensitive Data Redacted ***");
        formatter.field(
            "sse_kms_encryption_context",
            &"*** Sensitive Data Redacted ***",
        );
        formatter.field("bucket_key_enabled", &self.bucket_key_enabled);
        formatter.field("request_charged", &self.request_charged);
        formatter.field("upload_id", &self.upload_id);
        formatter.finish()
    }
}

/// A builder for [`UploadResponse`].
#[non_exhaustive]
#[derive(Default)]
pub struct UploadResponseBuilder {
    pub(crate) expiration: Option<String>,
    pub(crate) e_tag: Option<String>,
    pub(crate) checksum_crc32: Option<String>,
    pub(crate) checksum_crc32_c: Option<String>,
    pub(crate) checksum_sha1: Option<String>,
    pub(crate) checksum_sha256: Option<String>,
    pub(crate) server_side_encryption: Option<aws_sdk_s3::types::ServerSideEncryption>,
    pub(crate) version_id: Option<String>,
    pub(crate) sse_customer_algorithm: Option<String>,
    pub(crate) sse_customer_key_md5: Option<String>,
    pub(crate) sse_kms_key_id: Option<String>,
    pub(crate) sse_kms_encryption_context: Option<String>,
    pub(crate) bucket_key_enabled: Option<bool>,
    pub(crate) request_charged: Option<aws_sdk_s3::types::RequestCharged>,
    pub(crate) upload_id: Option<String>,
}

impl UploadResponseBuilder {
    /// <p>If the expiration is configured for the object (see <a href="https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutBucketLifecycleConfiguration.html">PutBucketLifecycleConfiguration</a>) in the <i>Amazon S3 User Guide</i>, the response includes this header. It includes the <code>expiry-date</code> and <code>rule-id</code> key-value pairs that provide information about object expiration. The value of the <code>rule-id</code> is URL-encoded.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn expiration(mut self, input: impl Into<String>) -> Self {
        self.expiration = Some(input.into());
        self
    }
    /// <p>If the expiration is configured for the object (see <a href="https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutBucketLifecycleConfiguration.html">PutBucketLifecycleConfiguration</a>) in the <i>Amazon S3 User Guide</i>, the response includes this header. It includes the <code>expiry-date</code> and <code>rule-id</code> key-value pairs that provide information about object expiration. The value of the <code>rule-id</code> is URL-encoded.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn set_expiration(mut self, input: Option<String>) -> Self {
        self.expiration = input;
        self
    }
    /// <p>If the expiration is configured for the object (see <a href="https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutBucketLifecycleConfiguration.html">PutBucketLifecycleConfiguration</a>) in the <i>Amazon S3 User Guide</i>, the response includes this header. It includes the <code>expiry-date</code> and <code>rule-id</code> key-value pairs that provide information about object expiration. The value of the <code>rule-id</code> is URL-encoded.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn get_expiration(&self) -> &Option<String> {
        &self.expiration
    }
    /// <p>Entity tag for the uploaded object.</p>
    /// <p><b>General purpose buckets </b> - To ensure that data is not corrupted traversing the network, for objects where the ETag is the MD5 digest of the object, you can calculate the MD5 while putting an object to Amazon S3 and compare the returned ETag to the calculated MD5 value.</p>
    /// <p><b>Directory buckets </b> - The ETag for the object in a directory bucket isn't the MD5 digest of the object.</p>
    pub fn e_tag(mut self, input: impl Into<String>) -> Self {
        self.e_tag = Some(input.into());
        self
    }
    /// <p>Entity tag for the uploaded object.</p>
    /// <p><b>General purpose buckets </b> - To ensure that data is not corrupted traversing the network, for objects where the ETag is the MD5 digest of the object, you can calculate the MD5 while putting an object to Amazon S3 and compare the returned ETag to the calculated MD5 value.</p>
    /// <p><b>Directory buckets </b> - The ETag for the object in a directory bucket isn't the MD5 digest of the object.</p>
    pub fn set_e_tag(mut self, input: Option<String>) -> Self {
        self.e_tag = input;
        self
    }
    /// <p>Entity tag for the uploaded object.</p>
    /// <p><b>General purpose buckets </b> - To ensure that data is not corrupted traversing the network, for objects where the ETag is the MD5 digest of the object, you can calculate the MD5 while putting an object to Amazon S3 and compare the returned ETag to the calculated MD5 value.</p>
    /// <p><b>Directory buckets </b> - The ETag for the object in a directory bucket isn't the MD5 digest of the object.</p>
    pub fn get_e_tag(&self) -> &Option<String> {
        &self.e_tag
    }
    /// <p>The base64-encoded, 32-bit CRC32 checksum of the object. This will only be present if it was uploaded with the object. When you use an API operation on an object that was uploaded using multipart uploads, this value may not be a direct checksum value of the full object. Instead, it's a calculation based on the checksum values of each individual part. For more information about how checksums are calculated with multipart uploads, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html#large-object-checksums"> Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn checksum_crc32(mut self, input: impl Into<String>) -> Self {
        self.checksum_crc32 = Some(input.into());
        self
    }
    /// <p>The base64-encoded, 32-bit CRC32 checksum of the object. This will only be present if it was uploaded with the object. When you use an API operation on an object that was uploaded using multipart uploads, this value may not be a direct checksum value of the full object. Instead, it's a calculation based on the checksum values of each individual part. For more information about how checksums are calculated with multipart uploads, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html#large-object-checksums"> Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn set_checksum_crc32(mut self, input: Option<String>) -> Self {
        self.checksum_crc32 = input;
        self
    }
    /// <p>The base64-encoded, 32-bit CRC32 checksum of the object. This will only be present if it was uploaded with the object. When you use an API operation on an object that was uploaded using multipart uploads, this value may not be a direct checksum value of the full object. Instead, it's a calculation based on the checksum values of each individual part. For more information about how checksums are calculated with multipart uploads, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html#large-object-checksums"> Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn get_checksum_crc32(&self) -> &Option<String> {
        &self.checksum_crc32
    }
    /// <p>The base64-encoded, 32-bit CRC32C checksum of the object. This will only be present if it was uploaded with the object. When you use an API operation on an object that was uploaded using multipart uploads, this value may not be a direct checksum value of the full object. Instead, it's a calculation based on the checksum values of each individual part. For more information about how checksums are calculated with multipart uploads, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html#large-object-checksums"> Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn checksum_crc32_c(mut self, input: impl Into<String>) -> Self {
        self.checksum_crc32_c = Some(input.into());
        self
    }
    /// <p>The base64-encoded, 32-bit CRC32C checksum of the object. This will only be present if it was uploaded with the object. When you use an API operation on an object that was uploaded using multipart uploads, this value may not be a direct checksum value of the full object. Instead, it's a calculation based on the checksum values of each individual part. For more information about how checksums are calculated with multipart uploads, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html#large-object-checksums"> Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn set_checksum_crc32_c(mut self, input: Option<String>) -> Self {
        self.checksum_crc32_c = input;
        self
    }
    /// <p>The base64-encoded, 32-bit CRC32C checksum of the object. This will only be present if it was uploaded with the object. When you use an API operation on an object that was uploaded using multipart uploads, this value may not be a direct checksum value of the full object. Instead, it's a calculation based on the checksum values of each individual part. For more information about how checksums are calculated with multipart uploads, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html#large-object-checksums"> Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn get_checksum_crc32_c(&self) -> &Option<String> {
        &self.checksum_crc32_c
    }
    /// <p>The base64-encoded, 160-bit SHA-1 digest of the object. This will only be present if it was uploaded with the object. When you use the API operation on an object that was uploaded using multipart uploads, this value may not be a direct checksum value of the full object. Instead, it's a calculation based on the checksum values of each individual part. For more information about how checksums are calculated with multipart uploads, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html#large-object-checksums"> Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn checksum_sha1(mut self, input: impl Into<String>) -> Self {
        self.checksum_sha1 = Some(input.into());
        self
    }
    /// <p>The base64-encoded, 160-bit SHA-1 digest of the object. This will only be present if it was uploaded with the object. When you use the API operation on an object that was uploaded using multipart uploads, this value may not be a direct checksum value of the full object. Instead, it's a calculation based on the checksum values of each individual part. For more information about how checksums are calculated with multipart uploads, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html#large-object-checksums"> Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn set_checksum_sha1(mut self, input: Option<String>) -> Self {
        self.checksum_sha1 = input;
        self
    }
    /// <p>The base64-encoded, 160-bit SHA-1 digest of the object. This will only be present if it was uploaded with the object. When you use the API operation on an object that was uploaded using multipart uploads, this value may not be a direct checksum value of the full object. Instead, it's a calculation based on the checksum values of each individual part. For more information about how checksums are calculated with multipart uploads, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html#large-object-checksums"> Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn get_checksum_sha1(&self) -> &Option<String> {
        &self.checksum_sha1
    }
    /// <p>The base64-encoded, 256-bit SHA-256 digest of the object. This will only be present if it was uploaded with the object. When you use an API operation on an object that was uploaded using multipart uploads, this value may not be a direct checksum value of the full object. Instead, it's a calculation based on the checksum values of each individual part. For more information about how checksums are calculated with multipart uploads, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html#large-object-checksums"> Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn checksum_sha256(mut self, input: impl Into<String>) -> Self {
        self.checksum_sha256 = Some(input.into());
        self
    }
    /// <p>The base64-encoded, 256-bit SHA-256 digest of the object. This will only be present if it was uploaded with the object. When you use an API operation on an object that was uploaded using multipart uploads, this value may not be a direct checksum value of the full object. Instead, it's a calculation based on the checksum values of each individual part. For more information about how checksums are calculated with multipart uploads, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html#large-object-checksums"> Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn set_checksum_sha256(mut self, input: Option<String>) -> Self {
        self.checksum_sha256 = input;
        self
    }
    /// <p>The base64-encoded, 256-bit SHA-256 digest of the object. This will only be present if it was uploaded with the object. When you use an API operation on an object that was uploaded using multipart uploads, this value may not be a direct checksum value of the full object. Instead, it's a calculation based on the checksum values of each individual part. For more information about how checksums are calculated with multipart uploads, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html#large-object-checksums"> Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn get_checksum_sha256(&self) -> &Option<String> {
        &self.checksum_sha256
    }
    /// <p>The server-side encryption algorithm used when you store this object in Amazon S3 (for example, <code>AES256</code>, <code>aws:kms</code>, <code>aws:kms:dsse</code>).</p><note>
    /// <p>For directory buckets, only server-side encryption with Amazon S3 managed keys (SSE-S3) (<code>AES256</code>) is supported.</p>
    /// </note>
    pub fn server_side_encryption(
        mut self,
        input: aws_sdk_s3::types::ServerSideEncryption,
    ) -> Self {
        self.server_side_encryption = Some(input);
        self
    }
    /// <p>The server-side encryption algorithm used when you store this object in Amazon S3 (for example, <code>AES256</code>, <code>aws:kms</code>, <code>aws:kms:dsse</code>).</p><note>
    /// <p>For directory buckets, only server-side encryption with Amazon S3 managed keys (SSE-S3) (<code>AES256</code>) is supported.</p>
    /// </note>
    pub fn set_server_side_encryption(
        mut self,
        input: Option<aws_sdk_s3::types::ServerSideEncryption>,
    ) -> Self {
        self.server_side_encryption = input;
        self
    }
    /// <p>The server-side encryption algorithm used when you store this object in Amazon S3 (for example, <code>AES256</code>, <code>aws:kms</code>, <code>aws:kms:dsse</code>).</p><note>
    /// <p>For directory buckets, only server-side encryption with Amazon S3 managed keys (SSE-S3) (<code>AES256</code>) is supported.</p>
    /// </note>
    pub fn get_server_side_encryption(&self) -> &Option<aws_sdk_s3::types::ServerSideEncryption> {
        &self.server_side_encryption
    }
    /// <p>Version ID of the object.</p>
    /// <p>If you enable versioning for a bucket, Amazon S3 automatically generates a unique version ID for the object being stored. Amazon S3 returns this ID in the response. When you enable versioning for a bucket, if Amazon S3 receives multiple write requests for the same object simultaneously, it stores all of the objects. For more information about versioning, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/AddingObjectstoVersioningEnabledBuckets.html">Adding Objects to Versioning-Enabled Buckets</a> in the <i>Amazon S3 User Guide</i>. For information about returning the versioning state of a bucket, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketVersioning.html">GetBucketVersioning</a>.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn version_id(mut self, input: impl Into<String>) -> Self {
        self.version_id = Some(input.into());
        self
    }
    /// <p>Version ID of the object.</p>
    /// <p>If you enable versioning for a bucket, Amazon S3 automatically generates a unique version ID for the object being stored. Amazon S3 returns this ID in the response. When you enable versioning for a bucket, if Amazon S3 receives multiple write requests for the same object simultaneously, it stores all of the objects. For more information about versioning, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/AddingObjectstoVersioningEnabledBuckets.html">Adding Objects to Versioning-Enabled Buckets</a> in the <i>Amazon S3 User Guide</i>. For information about returning the versioning state of a bucket, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketVersioning.html">GetBucketVersioning</a>.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn set_version_id(mut self, input: Option<String>) -> Self {
        self.version_id = input;
        self
    }
    /// <p>Version ID of the object.</p>
    /// <p>If you enable versioning for a bucket, Amazon S3 automatically generates a unique version ID for the object being stored. Amazon S3 returns this ID in the response. When you enable versioning for a bucket, if Amazon S3 receives multiple write requests for the same object simultaneously, it stores all of the objects. For more information about versioning, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/AddingObjectstoVersioningEnabledBuckets.html">Adding Objects to Versioning-Enabled Buckets</a> in the <i>Amazon S3 User Guide</i>. For information about returning the versioning state of a bucket, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketVersioning.html">GetBucketVersioning</a>.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn get_version_id(&self) -> &Option<String> {
        &self.version_id
    }
    /// <p>If server-side encryption with a customer-provided encryption key was requested, the response will include this header to confirm the encryption algorithm that's used.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn sse_customer_algorithm(mut self, input: impl Into<String>) -> Self {
        self.sse_customer_algorithm = Some(input.into());
        self
    }
    /// <p>If server-side encryption with a customer-provided encryption key was requested, the response will include this header to confirm the encryption algorithm that's used.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn set_sse_customer_algorithm(mut self, input: Option<String>) -> Self {
        self.sse_customer_algorithm = input;
        self
    }
    /// <p>If server-side encryption with a customer-provided encryption key was requested, the response will include this header to confirm the encryption algorithm that's used.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn get_sse_customer_algorithm(&self) -> &Option<String> {
        &self.sse_customer_algorithm
    }
    /// <p>If server-side encryption with a customer-provided encryption key was requested, the response will include this header to provide the round-trip message integrity verification of the customer-provided encryption key.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn sse_customer_key_md5(mut self, input: impl Into<String>) -> Self {
        self.sse_customer_key_md5 = Some(input.into());
        self
    }
    /// <p>If server-side encryption with a customer-provided encryption key was requested, the response will include this header to provide the round-trip message integrity verification of the customer-provided encryption key.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn set_sse_customer_key_md5(mut self, input: Option<String>) -> Self {
        self.sse_customer_key_md5 = input;
        self
    }
    /// <p>If server-side encryption with a customer-provided encryption key was requested, the response will include this header to provide the round-trip message integrity verification of the customer-provided encryption key.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn get_sse_customer_key_md5(&self) -> &Option<String> {
        &self.sse_customer_key_md5
    }
    /// <p>If <code>x-amz-server-side-encryption</code> has a valid value of <code>aws:kms</code> or <code>aws:kms:dsse</code>, this header indicates the ID of the Key Management Service (KMS) symmetric encryption customer managed key that was used for the object.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn sse_kms_key_id(mut self, input: impl Into<String>) -> Self {
        self.sse_kms_key_id = Some(input.into());
        self
    }
    /// <p>If <code>x-amz-server-side-encryption</code> has a valid value of <code>aws:kms</code> or <code>aws:kms:dsse</code>, this header indicates the ID of the Key Management Service (KMS) symmetric encryption customer managed key that was used for the object.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn set_sse_kms_key_id(mut self, input: Option<String>) -> Self {
        self.sse_kms_key_id = input;
        self
    }
    /// <p>If <code>x-amz-server-side-encryption</code> has a valid value of <code>aws:kms</code> or <code>aws:kms:dsse</code>, this header indicates the ID of the Key Management Service (KMS) symmetric encryption customer managed key that was used for the object.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn get_sse_kms_key_id(&self) -> &Option<String> {
        &self.sse_kms_key_id
    }
    /// <p>If present, indicates the Amazon Web Services KMS Encryption Context to use for object encryption. The value of this header is a base64-encoded UTF-8 string holding JSON with the encryption context key-value pairs. This value is stored as object metadata and automatically gets passed on to Amazon Web Services KMS for future <code>GetObject</code> or <code>CopyObject</code> operations on this object.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn sse_kms_encryption_context(mut self, input: impl Into<String>) -> Self {
        self.sse_kms_encryption_context = Some(input.into());
        self
    }
    /// <p>If present, indicates the Amazon Web Services KMS Encryption Context to use for object encryption. The value of this header is a base64-encoded UTF-8 string holding JSON with the encryption context key-value pairs. This value is stored as object metadata and automatically gets passed on to Amazon Web Services KMS for future <code>GetObject</code> or <code>CopyObject</code> operations on this object.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn set_sse_kms_encryption_context(mut self, input: Option<String>) -> Self {
        self.sse_kms_encryption_context = input;
        self
    }
    /// <p>If present, indicates the Amazon Web Services KMS Encryption Context to use for object encryption. The value of this header is a base64-encoded UTF-8 string holding JSON with the encryption context key-value pairs. This value is stored as object metadata and automatically gets passed on to Amazon Web Services KMS for future <code>GetObject</code> or <code>CopyObject</code> operations on this object.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn get_sse_kms_encryption_context(&self) -> &Option<String> {
        &self.sse_kms_encryption_context
    }
    /// <p>Indicates whether the uploaded object uses an S3 Bucket Key for server-side encryption with Key Management Service (KMS) keys (SSE-KMS).</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn bucket_key_enabled(mut self, input: bool) -> Self {
        self.bucket_key_enabled = Some(input);
        self
    }
    /// <p>Indicates whether the uploaded object uses an S3 Bucket Key for server-side encryption with Key Management Service (KMS) keys (SSE-KMS).</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn set_bucket_key_enabled(mut self, input: Option<bool>) -> Self {
        self.bucket_key_enabled = input;
        self
    }
    /// <p>Indicates whether the uploaded object uses an S3 Bucket Key for server-side encryption with Key Management Service (KMS) keys (SSE-KMS).</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn get_bucket_key_enabled(&self) -> &Option<bool> {
        &self.bucket_key_enabled
    }
    /// <p>If present, indicates that the requester was successfully charged for the request.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn request_charged(mut self, input: aws_sdk_s3::types::RequestCharged) -> Self {
        self.request_charged = Some(input);
        self
    }
    /// <p>If present, indicates that the requester was successfully charged for the request.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn set_request_charged(mut self, input: Option<aws_sdk_s3::types::RequestCharged>) -> Self {
        self.request_charged = input;
        self
    }
    /// <p>If present, indicates that the requester was successfully charged for the request.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn get_request_charged(&self) -> &Option<aws_sdk_s3::types::RequestCharged> {
        &self.request_charged
    }

    /// <p>ID for the initiated multipart upload.</p>
    pub fn upload_id(mut self, input: impl Into<String>) -> Self {
        self.upload_id = Some(input.into());
        self
    }
    /// <p>ID for the initiated multipart upload.</p>
    pub fn set_upload_id(mut self, input: Option<String>) -> Self {
        self.upload_id = input;
        self
    }
    /// <p>ID for the initiated multipart upload.</p>
    pub fn get_upload_id(&self) -> &Option<String> {
        &self.upload_id
    }

    /// Consumes the builder and constructs a [`UploadResponse`]
    // FIXME(aws-sdk-rust#1159): replace BuildError with our own type?
    pub fn build(self) -> Result<UploadResponse, ::aws_smithy_types::error::operation::BuildError> {
        Ok(UploadResponse {
            expiration: self.expiration,
            e_tag: self.e_tag,
            checksum_crc32: self.checksum_crc32,
            checksum_crc32_c: self.checksum_crc32_c,
            checksum_sha1: self.checksum_sha1,
            checksum_sha256: self.checksum_sha256,
            server_side_encryption: self.server_side_encryption,
            version_id: self.version_id,
            sse_customer_algorithm: self.sse_customer_algorithm,
            sse_customer_key_md5: self.sse_customer_key_md5,
            sse_kms_key_id: self.sse_kms_key_id,
            sse_kms_encryption_context: self.sse_kms_encryption_context,
            bucket_key_enabled: self.bucket_key_enabled,
            request_charged: self.request_charged,
            upload_id: self.upload_id,
        })
    }
}

impl Debug for UploadResponseBuilder {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        let mut formatter = f.debug_struct("UploadResponseBuilder");
        formatter.field("expiration", &self.expiration);
        formatter.field("e_tag", &self.e_tag);
        formatter.field("checksum_crc32", &self.checksum_crc32);
        formatter.field("checksum_crc32_c", &self.checksum_crc32_c);
        formatter.field("checksum_sha1", &self.checksum_sha1);
        formatter.field("checksum_sha256", &self.checksum_sha256);
        formatter.field("server_side_encryption", &self.server_side_encryption);
        formatter.field("version_id", &self.version_id);
        formatter.field("sse_customer_algorithm", &self.sse_customer_algorithm);
        formatter.field("sse_customer_key_md5", &self.sse_customer_key_md5);
        formatter.field("sse_kms_key_id", &"*** Sensitive Data Redacted ***");
        formatter.field(
            "sse_kms_encryption_context",
            &"*** Sensitive Data Redacted ***",
        );
        formatter.field("bucket_key_enabled", &self.bucket_key_enabled);
        formatter.field("request_charged", &self.request_charged);
        formatter.field("upload_id", &self.upload_id);
        formatter.finish()
    }
}

impl From<CreateMultipartUploadOutput> for UploadResponseBuilder {
    fn from(value: CreateMultipartUploadOutput) -> Self {
        UploadResponseBuilder {
            upload_id: value.upload_id,
            server_side_encryption: value.server_side_encryption,
            sse_customer_algorithm: value.sse_customer_algorithm,
            sse_customer_key_md5: value.sse_customer_key_md5,
            sse_kms_key_id: value.ssekms_key_id,
            sse_kms_encryption_context: value.ssekms_encryption_context,
            bucket_key_enabled: value.bucket_key_enabled,
            request_charged: value.request_charged,
            // remaining fields not available from CreateMultipartUploadOutput
            checksum_sha256: None,
            expiration: None,
            e_tag: None,
            checksum_crc32: None,
            checksum_crc32_c: None,
            checksum_sha1: None,
            version_id: None,
            // TODO(aws-sdk-rust#1159): abort_rule_id and abort_date seem unique to CreateMultipartUploadOutput
        }
    }
}
