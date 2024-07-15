/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::io::InputStream;
use std::fmt::Debug;
use std::mem;

/// Request type for uploading a single object
#[non_exhaustive]
pub struct UploadRequest {
    /// <p>The canned ACL to apply to the object. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/acl-overview.html#CannedACL">Canned ACL</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p>When adding a new object, you can use headers to grant ACL-based permissions to individual Amazon Web Services accounts or to predefined groups defined by Amazon S3. These permissions are then added to the ACL on the object. By default, all objects are private. Only the owner has full access control. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/acl-overview.html">Access Control List (ACL) Overview</a> and <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/acl-using-rest-api.html">Managing ACLs Using the REST API</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p>If the bucket that you're uploading objects to uses the bucket owner enforced setting for S3 Object Ownership, ACLs are disabled and no longer affect permissions. Buckets that use this setting only accept PUT requests that don't specify an ACL or PUT requests that specify bucket owner full control ACLs, such as the <code>bucket-owner-full-control</code> canned ACL or an equivalent form of this ACL expressed in the XML format. PUT requests that contain other ACLs (for example, custom grants to certain Amazon Web Services accounts) fail and return a <code>400</code> error with the error code <code>AccessControlListNotSupported</code>. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/about-object-ownership.html"> Controlling ownership of objects and disabling ACLs</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <ul>
    /// <li>
    /// <p>This functionality is not supported for directory buckets.</p></li>
    /// <li>
    /// <p>This functionality is not supported for Amazon S3 on Outposts.</p></li>
    /// </ul>
    /// </note>
    pub acl: Option<aws_sdk_s3::types::ObjectCannedAcl>,
    /// <p>Object data.</p>
    pub body: crate::io::InputStream,
    /// <p>The bucket name to which the PUT action was initiated.</p>
    /// <p><b>Directory buckets</b> - When you use this operation with a directory bucket, you must use virtual-hosted-style requests in the format <code> <i>Bucket_name</i>.s3express-<i>az_id</i>.<i>region</i>.amazonaws.com</code>. Path-style requests are not supported. Directory bucket names must be unique in the chosen Availability Zone. Bucket names must follow the format <code> <i>bucket_base_name</i>--<i>az-id</i>--x-s3</code> (for example, <code> <i>DOC-EXAMPLE-BUCKET</i>--<i>usw2-az1</i>--x-s3</code>). For information about bucket naming restrictions, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/directory-bucket-naming-rules.html">Directory bucket naming rules</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p><b>Access points</b> - When you use this action with an access point, you must provide the alias of the access point in place of the bucket name or specify the access point ARN. When using the access point ARN, you must direct requests to the access point hostname. The access point hostname takes the form <i>AccessPointName</i>-<i>AccountId</i>.s3-accesspoint.<i>Region</i>.amazonaws.com. When using this action with an access point through the Amazon Web Services SDKs, you provide the access point ARN in place of the bucket name. For more information about access point ARNs, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/using-access-points.html">Using access points</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <p>Access points and Object Lambda access points are not supported by directory buckets.</p>
    /// </note>
    /// <p><b>S3 on Outposts</b> - When you use this action with Amazon S3 on Outposts, you must direct requests to the S3 on Outposts hostname. The S3 on Outposts hostname takes the form <code> <i>AccessPointName</i>-<i>AccountId</i>.<i>outpostID</i>.s3-outposts.<i>Region</i>.amazonaws.com</code>. When you use this action with S3 on Outposts through the Amazon Web Services SDKs, you provide the Outposts access point ARN in place of the bucket name. For more information about S3 on Outposts ARNs, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/S3onOutposts.html">What is S3 on Outposts?</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub bucket: Option<String>,
    /// <p>Can be used to specify caching behavior along the request/reply chain. For more information, see <a href="http://www.w3.org/Protocols/rfc2616/rfc2616-sec14.html#sec14.9">http://www.w3.org/Protocols/rfc2616/rfc2616-sec14.html#sec14.9</a>.</p>
    pub cache_control: Option<String>,
    /// <p>Specifies presentational information for the object. For more information, see <a href="https://www.rfc-editor.org/rfc/rfc6266#section-4">https://www.rfc-editor.org/rfc/rfc6266#section-4</a>.</p>
    pub content_disposition: Option<String>,
    /// <p>Specifies what content encodings have been applied to the object and thus what decoding mechanisms must be applied to obtain the media-type referenced by the Content-Type header field. For more information, see <a href="https://www.rfc-editor.org/rfc/rfc9110.html#field.content-encoding">https://www.rfc-editor.org/rfc/rfc9110.html#field.content-encoding</a>.</p>
    pub content_encoding: Option<String>,
    /// <p>The language the content is in.</p>
    pub content_language: Option<String>,
    /// <p>Size of the body in bytes. This parameter is useful when the size of the body cannot be determined automatically. For more information, see <a href="https://www.rfc-editor.org/rfc/rfc9110.html#name-content-length">https://www.rfc-editor.org/rfc/rfc9110.html#name-content-length</a>.</p>
    pub content_length: Option<i64>,
    /// <p>The base64-encoded 128-bit MD5 digest of the message (without the headers) according to RFC 1864. This header can be used as a message integrity check to verify that the data is the same data that was originally sent. Although it is optional, we recommend using the Content-MD5 mechanism as an end-to-end integrity check. For more information about REST request authentication, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/RESTAuthentication.html">REST Authentication</a>.</p><note>
    /// <p>The <code>Content-MD5</code> header is required for any request to upload an object with a retention period configured using Amazon S3 Object Lock. For more information about Amazon S3 Object Lock, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/object-lock-overview.html">Amazon S3 Object Lock Overview</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// </note> <note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub content_md5: Option<String>,
    /// <p>A standard MIME type describing the format of the contents. For more information, see <a href="https://www.rfc-editor.org/rfc/rfc9110.html#name-content-type">https://www.rfc-editor.org/rfc/rfc9110.html#name-content-type</a>.</p>
    pub content_type: Option<String>,
    /// <p>Indicates the algorithm used to create the checksum for the object when you use the SDK. This header will not provide any additional functionality if you don't use the SDK. When you send this header, there must be a corresponding <code>x-amz-checksum-<i>algorithm</i> </code> or <code>x-amz-trailer</code> header sent. Otherwise, Amazon S3 fails the request with the HTTP status code <code>400 Bad Request</code>.</p>
    /// <p>For the <code>x-amz-checksum-<i>algorithm</i> </code> header, replace <code> <i>algorithm</i> </code> with the supported algorithm from the following list:</p>
    /// <ul>
    /// <li>
    /// <p>CRC32</p></li>
    /// <li>
    /// <p>CRC32C</p></li>
    /// <li>
    /// <p>SHA1</p></li>
    /// <li>
    /// <p>SHA256</p></li>
    /// </ul>
    /// <p>For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html">Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p>If the individual checksum value you provide through <code>x-amz-checksum-<i>algorithm</i> </code> doesn't match the checksum algorithm you set through <code>x-amz-sdk-checksum-algorithm</code>, Amazon S3 ignores any provided <code>ChecksumAlgorithm</code> parameter and uses the checksum algorithm that matches the provided value in <code>x-amz-checksum-<i>algorithm</i> </code>.</p><note>
    /// <p>For directory buckets, when you use Amazon Web Services SDKs, <code>CRC32</code> is the default checksum algorithm that's used for performance.</p>
    /// </note>
    pub checksum_algorithm: Option<aws_sdk_s3::types::ChecksumAlgorithm>,
    /// <p>This header can be used as a data integrity check to verify that the data received is the same data that was originally sent. This header specifies the base64-encoded, 32-bit CRC32 checksum of the object. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html">Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub checksum_crc32: Option<String>,
    /// <p>This header can be used as a data integrity check to verify that the data received is the same data that was originally sent. This header specifies the base64-encoded, 32-bit CRC32C checksum of the object. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html">Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub checksum_crc32_c: Option<String>,
    /// <p>This header can be used as a data integrity check to verify that the data received is the same data that was originally sent. This header specifies the base64-encoded, 160-bit SHA-1 digest of the object. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html">Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub checksum_sha1: Option<String>,
    /// <p>This header can be used as a data integrity check to verify that the data received is the same data that was originally sent. This header specifies the base64-encoded, 256-bit SHA-256 digest of the object. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html">Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub checksum_sha256: Option<String>,
    /// <p>The date and time at which the object is no longer cacheable. For more information, see <a href="https://www.rfc-editor.org/rfc/rfc7234#section-5.3">https://www.rfc-editor.org/rfc/rfc7234#section-5.3</a>.</p>
    pub expires: Option<::aws_smithy_types::DateTime>,
    /// <p>Gives the grantee READ, READ_ACP, and WRITE_ACP permissions on the object.</p><note>
    /// <ul>
    /// <li>
    /// <p>This functionality is not supported for directory buckets.</p></li>
    /// <li>
    /// <p>This functionality is not supported for Amazon S3 on Outposts.</p></li>
    /// </ul>
    /// </note>
    pub grant_full_control: Option<String>,
    /// <p>Allows grantee to read the object data and its metadata.</p><note>
    /// <ul>
    /// <li>
    /// <p>This functionality is not supported for directory buckets.</p></li>
    /// <li>
    /// <p>This functionality is not supported for Amazon S3 on Outposts.</p></li>
    /// </ul>
    /// </note>
    pub grant_read: Option<String>,
    /// <p>Allows grantee to read the object ACL.</p><note>
    /// <ul>
    /// <li>
    /// <p>This functionality is not supported for directory buckets.</p></li>
    /// <li>
    /// <p>This functionality is not supported for Amazon S3 on Outposts.</p></li>
    /// </ul>
    /// </note>
    pub grant_read_acp: Option<String>,
    /// <p>Allows grantee to write the ACL for the applicable object.</p><note>
    /// <ul>
    /// <li>
    /// <p>This functionality is not supported for directory buckets.</p></li>
    /// <li>
    /// <p>This functionality is not supported for Amazon S3 on Outposts.</p></li>
    /// </ul>
    /// </note>
    pub grant_write_acp: Option<String>,
    /// <p>Object key for which the PUT action was initiated.</p>
    pub key: Option<String>,
    /// <p>A map of metadata to store with the object in S3.</p>
    pub metadata: Option<::std::collections::HashMap<String, String>>,
    /// <p>The server-side encryption algorithm that was used when you store this object in Amazon S3 (for example, <code>AES256</code>, <code>aws:kms</code>, <code>aws:kms:dsse</code>).</p>
    /// <p><b>General purpose buckets </b> - You have four mutually exclusive options to protect data using server-side encryption in Amazon S3, depending on how you choose to manage the encryption keys. Specifically, the encryption key options are Amazon S3 managed keys (SSE-S3), Amazon Web Services KMS keys (SSE-KMS or DSSE-KMS), and customer-provided keys (SSE-C). Amazon S3 encrypts data with server-side encryption by using Amazon S3 managed keys (SSE-S3) by default. You can optionally tell Amazon S3 to encrypt data at rest by using server-side encryption with other key options. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/UsingServerSideEncryption.html">Using Server-Side Encryption</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p><b>Directory buckets </b> - For directory buckets, only the server-side encryption with Amazon S3 managed keys (SSE-S3) (<code>AES256</code>) value is supported.</p>
    pub server_side_encryption: Option<aws_sdk_s3::types::ServerSideEncryption>,
    /// <p>By default, Amazon S3 uses the STANDARD Storage Class to store newly created objects. The STANDARD storage class provides high durability and high availability. Depending on performance needs, you can specify a different Storage Class. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/storage-class-intro.html">Storage Classes</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <ul>
    /// <li>
    /// <p>For directory buckets, only the S3 Express One Zone storage class is supported to store newly created objects.</p></li>
    /// <li>
    /// <p>Amazon S3 on Outposts only uses the OUTPOSTS Storage Class.</p></li>
    /// </ul>
    /// </note>
    pub storage_class: Option<aws_sdk_s3::types::StorageClass>,
    /// <p>If the bucket is configured as a website, redirects requests for this object to another object in the same bucket or to an external URL. Amazon S3 stores the value of this header in the object metadata. For information about object metadata, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/UsingMetadata.html">Object Key and Metadata</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p>In the following example, the request header sets the redirect to an object (anotherPage.html) in the same bucket:</p>
    /// <p><code>x-amz-website-redirect-location: /anotherPage.html</code></p>
    /// <p>In the following example, the request header sets the object redirect to another website:</p>
    /// <p><code>x-amz-website-redirect-location: http://www.example.com/</code></p>
    /// <p>For more information about website hosting in Amazon S3, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/WebsiteHosting.html">Hosting Websites on Amazon S3</a> and <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/how-to-page-redirect.html">How to Configure Website Page Redirects</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub website_redirect_location: Option<String>,
    /// <p>Specifies the algorithm to use when encrypting the object (for example, <code>AES256</code>).</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub sse_customer_algorithm: Option<String>,
    /// <p>Specifies the customer-provided encryption key for Amazon S3 to use in encrypting data. This value is used to store the object and then it is discarded; Amazon S3 does not store the encryption key. The key must be appropriate for use with the algorithm specified in the <code>x-amz-server-side-encryption-customer-algorithm</code> header.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub sse_customer_key: Option<String>,
    /// <p>Specifies the 128-bit MD5 digest of the encryption key according to RFC 1321. Amazon S3 uses this header for a message integrity check to ensure that the encryption key was transmitted without error.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub sse_customer_key_md5: Option<String>,
    /// <p>If <code>x-amz-server-side-encryption</code> has a valid value of <code>aws:kms</code> or <code>aws:kms:dsse</code>, this header specifies the ID (Key ID, Key ARN, or Key Alias) of the Key Management Service (KMS) symmetric encryption customer managed key that was used for the object. If you specify <code>x-amz-server-side-encryption:aws:kms</code> or <code>x-amz-server-side-encryption:aws:kms:dsse</code>, but do not provide<code> x-amz-server-side-encryption-aws-kms-key-id</code>, Amazon S3 uses the Amazon Web Services managed key (<code>aws/s3</code>) to protect the data. If the KMS key does not exist in the same account that's issuing the command, you must use the full ARN and not just the ID.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub sse_kms_key_id: Option<String>,
    /// <p>Specifies the Amazon Web Services KMS Encryption Context to use for object encryption. The value of this header is a base64-encoded UTF-8 string holding JSON with the encryption context key-value pairs. This value is stored as object metadata and automatically gets passed on to Amazon Web Services KMS for future <code>GetObject</code> or <code>CopyObject</code> operations on this object. This value must be explicitly added during <code>CopyObject</code> operations.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub sse_kms_encryption_context: Option<String>,
    /// <p>Specifies whether Amazon S3 should use an S3 Bucket Key for object encryption with server-side encryption using Key Management Service (KMS) keys (SSE-KMS). Setting this header to <code>true</code> causes Amazon S3 to use an S3 Bucket Key for object encryption with SSE-KMS.</p>
    /// <p>Specifying this header with a PUT action doesn’t affect bucket-level settings for S3 Bucket Key.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub bucket_key_enabled: Option<bool>,
    /// <p>Confirms that the requester knows that they will be charged for the request. Bucket owners need not specify this parameter in their requests. If either the source or destination S3 bucket has Requester Pays enabled, the requester will pay for corresponding charges to copy the object. For information about downloading objects from Requester Pays buckets, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/ObjectsinRequesterPaysBuckets.html">Downloading Objects in Requester Pays Buckets</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub request_payer: Option<aws_sdk_s3::types::RequestPayer>,
    /// <p>The tag-set for the object. The tag-set must be encoded as URL Query parameters. (For example, "Key1=Value1")</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub tagging: Option<String>,
    /// <p>The Object Lock mode that you want to apply to this object.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub object_lock_mode: Option<aws_sdk_s3::types::ObjectLockMode>,
    /// <p>The date and time when you want this object's Object Lock to expire. Must be formatted as a timestamp parameter.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub object_lock_retain_until_date: Option<::aws_smithy_types::DateTime>,
    /// <p>Specifies whether a legal hold will be applied to this object. For more information about S3 Object Lock, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/object-lock.html">Object Lock</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub object_lock_legal_hold_status: Option<aws_sdk_s3::types::ObjectLockLegalHoldStatus>,
    /// <p>The account ID of the expected bucket owner. If the account ID that you provide does not match the actual owner of the bucket, the request fails with the HTTP status code <code>403 Forbidden</code> (access denied).</p>
    pub expected_bucket_owner: Option<String>,
}

impl UploadRequest {
    /// Create a new builder for `UploadRequest`
    pub fn builder() -> UploadRequestBuilder {
        UploadRequestBuilder::default()
    }

    /// Split the body from the request by taking it and replacing it with the default.
    pub(crate) fn take_body(&mut self) -> InputStream {
        mem::take(&mut self.body)
    }

    /// <p>The canned ACL to apply to the object. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/acl-overview.html#CannedACL">Canned ACL</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p>When adding a new object, you can use headers to grant ACL-based permissions to individual Amazon Web Services accounts or to predefined groups defined by Amazon S3. These permissions are then added to the ACL on the object. By default, all objects are private. Only the owner has full access control. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/acl-overview.html">Access Control List (ACL) Overview</a> and <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/acl-using-rest-api.html">Managing ACLs Using the REST API</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p>If the bucket that you're uploading objects to uses the bucket owner enforced setting for S3 Object Ownership, ACLs are disabled and no longer affect permissions. Buckets that use this setting only accept PUT requests that don't specify an ACL or PUT requests that specify bucket owner full control ACLs, such as the <code>bucket-owner-full-control</code> canned ACL or an equivalent form of this ACL expressed in the XML format. PUT requests that contain other ACLs (for example, custom grants to certain Amazon Web Services accounts) fail and return a <code>400</code> error with the error code <code>AccessControlListNotSupported</code>. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/about-object-ownership.html"> Controlling ownership of objects and disabling ACLs</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <ul>
    /// <li>
    /// <p>This functionality is not supported for directory buckets.</p></li>
    /// <li>
    /// <p>This functionality is not supported for Amazon S3 on Outposts.</p></li>
    /// </ul>
    /// </note>
    pub fn acl(&self) -> Option<&aws_sdk_s3::types::ObjectCannedAcl> {
        self.acl.as_ref()
    }
    /// <p>Object data.</p>
    pub fn body(&self) -> &crate::io::InputStream {
        &self.body
    }
    /// <p>The bucket name to which the PUT action was initiated.</p>
    /// <p><b>Directory buckets</b> - When you use this operation with a directory bucket, you must use virtual-hosted-style requests in the format <code> <i>Bucket_name</i>.s3express-<i>az_id</i>.<i>region</i>.amazonaws.com</code>. Path-style requests are not supported. Directory bucket names must be unique in the chosen Availability Zone. Bucket names must follow the format <code> <i>bucket_base_name</i>--<i>az-id</i>--x-s3</code> (for example, <code> <i>DOC-EXAMPLE-BUCKET</i>--<i>usw2-az1</i>--x-s3</code>). For information about bucket naming restrictions, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/directory-bucket-naming-rules.html">Directory bucket naming rules</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p><b>Access points</b> - When you use this action with an access point, you must provide the alias of the access point in place of the bucket name or specify the access point ARN. When using the access point ARN, you must direct requests to the access point hostname. The access point hostname takes the form <i>AccessPointName</i>-<i>AccountId</i>.s3-accesspoint.<i>Region</i>.amazonaws.com. When using this action with an access point through the Amazon Web Services SDKs, you provide the access point ARN in place of the bucket name. For more information about access point ARNs, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/using-access-points.html">Using access points</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <p>Access points and Object Lambda access points are not supported by directory buckets.</p>
    /// </note>
    /// <p><b>S3 on Outposts</b> - When you use this action with Amazon S3 on Outposts, you must direct requests to the S3 on Outposts hostname. The S3 on Outposts hostname takes the form <code> <i>AccessPointName</i>-<i>AccountId</i>.<i>outpostID</i>.s3-outposts.<i>Region</i>.amazonaws.com</code>. When you use this action with S3 on Outposts through the Amazon Web Services SDKs, you provide the Outposts access point ARN in place of the bucket name. For more information about S3 on Outposts ARNs, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/S3onOutposts.html">What is S3 on Outposts?</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn bucket(&self) -> Option<&str> {
        self.bucket.as_deref()
    }
    /// <p>Can be used to specify caching behavior along the request/reply chain. For more information, see <a href="http://www.w3.org/Protocols/rfc2616/rfc2616-sec14.html#sec14.9">http://www.w3.org/Protocols/rfc2616/rfc2616-sec14.html#sec14.9</a>.</p>
    pub fn cache_control(&self) -> Option<&str> {
        self.cache_control.as_deref()
    }
    /// <p>Specifies presentational information for the object. For more information, see <a href="https://www.rfc-editor.org/rfc/rfc6266#section-4">https://www.rfc-editor.org/rfc/rfc6266#section-4</a>.</p>
    pub fn content_disposition(&self) -> Option<&str> {
        self.content_disposition.as_deref()
    }
    /// <p>Specifies what content encodings have been applied to the object and thus what decoding mechanisms must be applied to obtain the media-type referenced by the Content-Type header field. For more information, see <a href="https://www.rfc-editor.org/rfc/rfc9110.html#field.content-encoding">https://www.rfc-editor.org/rfc/rfc9110.html#field.content-encoding</a>.</p>
    pub fn content_encoding(&self) -> Option<&str> {
        self.content_encoding.as_deref()
    }
    /// <p>The language the content is in.</p>
    pub fn content_language(&self) -> Option<&str> {
        self.content_language.as_deref()
    }
    /// <p>Size of the body in bytes. This parameter is useful when the size of the body cannot be determined automatically. For more information, see <a href="https://www.rfc-editor.org/rfc/rfc9110.html#name-content-length">https://www.rfc-editor.org/rfc/rfc9110.html#name-content-length</a>.</p>
    pub fn content_length(&self) -> Option<i64> {
        self.content_length
    }
    /// <p>The base64-encoded 128-bit MD5 digest of the message (without the headers) according to RFC 1864. This header can be used as a message integrity check to verify that the data is the same data that was originally sent. Although it is optional, we recommend using the Content-MD5 mechanism as an end-to-end integrity check. For more information about REST request authentication, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/RESTAuthentication.html">REST Authentication</a>.</p><note>
    /// <p>The <code>Content-MD5</code> header is required for any request to upload an object with a retention period configured using Amazon S3 Object Lock. For more information about Amazon S3 Object Lock, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/object-lock-overview.html">Amazon S3 Object Lock Overview</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// </note> <note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn content_md5(&self) -> Option<&str> {
        self.content_md5.as_deref()
    }
    /// <p>A standard MIME type describing the format of the contents. For more information, see <a href="https://www.rfc-editor.org/rfc/rfc9110.html#name-content-type">https://www.rfc-editor.org/rfc/rfc9110.html#name-content-type</a>.</p>
    pub fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }
    /// <p>Indicates the algorithm used to create the checksum for the object when you use the SDK. This header will not provide any additional functionality if you don't use the SDK. When you send this header, there must be a corresponding <code>x-amz-checksum-<i>algorithm</i> </code> or <code>x-amz-trailer</code> header sent. Otherwise, Amazon S3 fails the request with the HTTP status code <code>400 Bad Request</code>.</p>
    /// <p>For the <code>x-amz-checksum-<i>algorithm</i> </code> header, replace <code> <i>algorithm</i> </code> with the supported algorithm from the following list:</p>
    /// <ul>
    /// <li>
    /// <p>CRC32</p></li>
    /// <li>
    /// <p>CRC32C</p></li>
    /// <li>
    /// <p>SHA1</p></li>
    /// <li>
    /// <p>SHA256</p></li>
    /// </ul>
    /// <p>For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html">Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p>If the individual checksum value you provide through <code>x-amz-checksum-<i>algorithm</i> </code> doesn't match the checksum algorithm you set through <code>x-amz-sdk-checksum-algorithm</code>, Amazon S3 ignores any provided <code>ChecksumAlgorithm</code> parameter and uses the checksum algorithm that matches the provided value in <code>x-amz-checksum-<i>algorithm</i> </code>.</p><note>
    /// <p>For directory buckets, when you use Amazon Web Services SDKs, <code>CRC32</code> is the default checksum algorithm that's used for performance.</p>
    /// </note>
    pub fn checksum_algorithm(&self) -> Option<&aws_sdk_s3::types::ChecksumAlgorithm> {
        self.checksum_algorithm.as_ref()
    }
    /// <p>This header can be used as a data integrity check to verify that the data received is the same data that was originally sent. This header specifies the base64-encoded, 32-bit CRC32 checksum of the object. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html">Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn checksum_crc32(&self) -> Option<&str> {
        self.checksum_crc32.as_deref()
    }
    /// <p>This header can be used as a data integrity check to verify that the data received is the same data that was originally sent. This header specifies the base64-encoded, 32-bit CRC32C checksum of the object. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html">Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn checksum_crc32_c(&self) -> Option<&str> {
        self.checksum_crc32_c.as_deref()
    }
    /// <p>This header can be used as a data integrity check to verify that the data received is the same data that was originally sent. This header specifies the base64-encoded, 160-bit SHA-1 digest of the object. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html">Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn checksum_sha1(&self) -> Option<&str> {
        self.checksum_sha1.as_deref()
    }
    /// <p>This header can be used as a data integrity check to verify that the data received is the same data that was originally sent. This header specifies the base64-encoded, 256-bit SHA-256 digest of the object. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html">Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn checksum_sha256(&self) -> Option<&str> {
        self.checksum_sha256.as_deref()
    }
    /// <p>The date and time at which the object is no longer cacheable. For more information, see <a href="https://www.rfc-editor.org/rfc/rfc7234#section-5.3">https://www.rfc-editor.org/rfc/rfc7234#section-5.3</a>.</p>
    pub fn expires(&self) -> Option<&::aws_smithy_types::DateTime> {
        self.expires.as_ref()
    }
    /// <p>Gives the grantee READ, READ_ACP, and WRITE_ACP permissions on the object.</p><note>
    /// <ul>
    /// <li>
    /// <p>This functionality is not supported for directory buckets.</p></li>
    /// <li>
    /// <p>This functionality is not supported for Amazon S3 on Outposts.</p></li>
    /// </ul>
    /// </note>
    pub fn grant_full_control(&self) -> Option<&str> {
        self.grant_full_control.as_deref()
    }
    /// <p>Allows grantee to read the object data and its metadata.</p><note>
    /// <ul>
    /// <li>
    /// <p>This functionality is not supported for directory buckets.</p></li>
    /// <li>
    /// <p>This functionality is not supported for Amazon S3 on Outposts.</p></li>
    /// </ul>
    /// </note>
    pub fn grant_read(&self) -> Option<&str> {
        self.grant_read.as_deref()
    }
    /// <p>Allows grantee to read the object ACL.</p><note>
    /// <ul>
    /// <li>
    /// <p>This functionality is not supported for directory buckets.</p></li>
    /// <li>
    /// <p>This functionality is not supported for Amazon S3 on Outposts.</p></li>
    /// </ul>
    /// </note>
    pub fn grant_read_acp(&self) -> Option<&str> {
        self.grant_read_acp.as_deref()
    }
    /// <p>Allows grantee to write the ACL for the applicable object.</p><note>
    /// <ul>
    /// <li>
    /// <p>This functionality is not supported for directory buckets.</p></li>
    /// <li>
    /// <p>This functionality is not supported for Amazon S3 on Outposts.</p></li>
    /// </ul>
    /// </note>
    pub fn grant_write_acp(&self) -> Option<&str> {
        self.grant_write_acp.as_deref()
    }
    /// <p>Object key for which the PUT action was initiated.</p>
    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }
    /// <p>A map of metadata to store with the object in S3.</p>
    pub fn metadata(&self) -> Option<&::std::collections::HashMap<String, String>> {
        self.metadata.as_ref()
    }
    /// <p>The server-side encryption algorithm that was used when you store this object in Amazon S3 (for example, <code>AES256</code>, <code>aws:kms</code>, <code>aws:kms:dsse</code>).</p>
    /// <p><b>General purpose buckets </b> - You have four mutually exclusive options to protect data using server-side encryption in Amazon S3, depending on how you choose to manage the encryption keys. Specifically, the encryption key options are Amazon S3 managed keys (SSE-S3), Amazon Web Services KMS keys (SSE-KMS or DSSE-KMS), and customer-provided keys (SSE-C). Amazon S3 encrypts data with server-side encryption by using Amazon S3 managed keys (SSE-S3) by default. You can optionally tell Amazon S3 to encrypt data at rest by using server-side encryption with other key options. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/UsingServerSideEncryption.html">Using Server-Side Encryption</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p><b>Directory buckets </b> - For directory buckets, only the server-side encryption with Amazon S3 managed keys (SSE-S3) (<code>AES256</code>) value is supported.</p>
    pub fn server_side_encryption(&self) -> Option<&aws_sdk_s3::types::ServerSideEncryption> {
        self.server_side_encryption.as_ref()
    }
    /// <p>By default, Amazon S3 uses the STANDARD Storage Class to store newly created objects. The STANDARD storage class provides high durability and high availability. Depending on performance needs, you can specify a different Storage Class. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/storage-class-intro.html">Storage Classes</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <ul>
    /// <li>
    /// <p>For directory buckets, only the S3 Express One Zone storage class is supported to store newly created objects.</p></li>
    /// <li>
    /// <p>Amazon S3 on Outposts only uses the OUTPOSTS Storage Class.</p></li>
    /// </ul>
    /// </note>
    pub fn storage_class(&self) -> Option<&aws_sdk_s3::types::StorageClass> {
        self.storage_class.as_ref()
    }
    /// <p>If the bucket is configured as a website, redirects requests for this object to another object in the same bucket or to an external URL. Amazon S3 stores the value of this header in the object metadata. For information about object metadata, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/UsingMetadata.html">Object Key and Metadata</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p>In the following example, the request header sets the redirect to an object (anotherPage.html) in the same bucket:</p>
    /// <p><code>x-amz-website-redirect-location: /anotherPage.html</code></p>
    /// <p>In the following example, the request header sets the object redirect to another website:</p>
    /// <p><code>x-amz-website-redirect-location: http://www.example.com/</code></p>
    /// <p>For more information about website hosting in Amazon S3, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/WebsiteHosting.html">Hosting Websites on Amazon S3</a> and <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/how-to-page-redirect.html">How to Configure Website Page Redirects</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn website_redirect_location(&self) -> Option<&str> {
        self.website_redirect_location.as_deref()
    }
    /// <p>Specifies the algorithm to use when encrypting the object (for example, <code>AES256</code>).</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn sse_customer_algorithm(&self) -> Option<&str> {
        self.sse_customer_algorithm.as_deref()
    }
    /// <p>Specifies the customer-provided encryption key for Amazon S3 to use in encrypting data. This value is used to store the object and then it is discarded; Amazon S3 does not store the encryption key. The key must be appropriate for use with the algorithm specified in the <code>x-amz-server-side-encryption-customer-algorithm</code> header.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn sse_customer_key(&self) -> Option<&str> {
        self.sse_customer_key.as_deref()
    }
    /// <p>Specifies the 128-bit MD5 digest of the encryption key according to RFC 1321. Amazon S3 uses this header for a message integrity check to ensure that the encryption key was transmitted without error.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn sse_customer_key_md5(&self) -> Option<&str> {
        self.sse_customer_key_md5.as_deref()
    }
    /// <p>If <code>x-amz-server-side-encryption</code> has a valid value of <code>aws:kms</code> or <code>aws:kms:dsse</code>, this header specifies the ID (Key ID, Key ARN, or Key Alias) of the Key Management Service (KMS) symmetric encryption customer managed key that was used for the object. If you specify <code>x-amz-server-side-encryption:aws:kms</code> or <code>x-amz-server-side-encryption:aws:kms:dsse</code>, but do not provide<code> x-amz-server-side-encryption-aws-kms-key-id</code>, Amazon S3 uses the Amazon Web Services managed key (<code>aws/s3</code>) to protect the data. If the KMS key does not exist in the same account that's issuing the command, you must use the full ARN and not just the ID.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn sse_kms_key_id(&self) -> Option<&str> {
        self.sse_kms_key_id.as_deref()
    }
    /// <p>Specifies the Amazon Web Services KMS Encryption Context to use for object encryption. The value of this header is a base64-encoded UTF-8 string holding JSON with the encryption context key-value pairs. This value is stored as object metadata and automatically gets passed on to Amazon Web Services KMS for future <code>GetObject</code> or <code>CopyObject</code> operations on this object. This value must be explicitly added during <code>CopyObject</code> operations.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn sse_kms_encryption_context(&self) -> Option<&str> {
        self.sse_kms_encryption_context.as_deref()
    }
    /// <p>Specifies whether Amazon S3 should use an S3 Bucket Key for object encryption with server-side encryption using Key Management Service (KMS) keys (SSE-KMS). Setting this header to <code>true</code> causes Amazon S3 to use an S3 Bucket Key for object encryption with SSE-KMS.</p>
    /// <p>Specifying this header with a PUT action doesn’t affect bucket-level settings for S3 Bucket Key.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn bucket_key_enabled(&self) -> Option<bool> {
        self.bucket_key_enabled
    }
    /// <p>Confirms that the requester knows that they will be charged for the request. Bucket owners need not specify this parameter in their requests. If either the source or destination S3 bucket has Requester Pays enabled, the requester will pay for corresponding charges to copy the object. For information about downloading objects from Requester Pays buckets, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/ObjectsinRequesterPaysBuckets.html">Downloading Objects in Requester Pays Buckets</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn request_payer(&self) -> Option<&aws_sdk_s3::types::RequestPayer> {
        self.request_payer.as_ref()
    }
    /// <p>The tag-set for the object. The tag-set must be encoded as URL Query parameters. (For example, "Key1=Value1")</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn tagging(&self) -> Option<&str> {
        self.tagging.as_deref()
    }
    /// <p>The Object Lock mode that you want to apply to this object.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn object_lock_mode(&self) -> Option<&aws_sdk_s3::types::ObjectLockMode> {
        self.object_lock_mode.as_ref()
    }
    /// <p>The date and time when you want this object's Object Lock to expire. Must be formatted as a timestamp parameter.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn object_lock_retain_until_date(&self) -> Option<&::aws_smithy_types::DateTime> {
        self.object_lock_retain_until_date.as_ref()
    }
    /// <p>Specifies whether a legal hold will be applied to this object. For more information about S3 Object Lock, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/object-lock.html">Object Lock</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn object_lock_legal_hold_status(
        &self,
    ) -> Option<&aws_sdk_s3::types::ObjectLockLegalHoldStatus> {
        self.object_lock_legal_hold_status.as_ref()
    }
    /// <p>The account ID of the expected bucket owner. If the account ID that you provide does not match the actual owner of the bucket, the request fails with the HTTP status code <code>403 Forbidden</code> (access denied).</p>
    pub fn expected_bucket_owner(&self) -> Option<&str> {
        self.expected_bucket_owner.as_deref()
    }
}

impl Debug for UploadRequest {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        let mut formatter = f.debug_struct("UploadRequest");
        formatter.field("acl", &self.acl);
        formatter.field("body", &self.body);
        formatter.field("bucket", &self.bucket);
        formatter.field("cache_control", &self.cache_control);
        formatter.field("content_disposition", &self.content_disposition);
        formatter.field("content_encoding", &self.content_encoding);
        formatter.field("content_language", &self.content_language);
        formatter.field("content_length", &self.content_length);
        formatter.field("content_md5", &self.content_md5);
        formatter.field("content_type", &self.content_type);
        formatter.field("checksum_algorithm", &self.checksum_algorithm);
        formatter.field("checksum_crc32", &self.checksum_crc32);
        formatter.field("checksum_crc32_c", &self.checksum_crc32_c);
        formatter.field("checksum_sha1", &self.checksum_sha1);
        formatter.field("checksum_sha256", &self.checksum_sha256);
        formatter.field("expires", &self.expires);
        formatter.field("grant_full_control", &self.grant_full_control);
        formatter.field("grant_read", &self.grant_read);
        formatter.field("grant_read_acp", &self.grant_read_acp);
        formatter.field("grant_write_acp", &self.grant_write_acp);
        formatter.field("key", &self.key);
        formatter.field("metadata", &self.metadata);
        formatter.field("server_side_encryption", &self.server_side_encryption);
        formatter.field("storage_class", &self.storage_class);
        formatter.field(
            "website_redirect_location",
            &self.website_redirect_location(),
        );
        formatter.field("sse_customer_algorithm", &self.sse_customer_algorithm);
        formatter.field("sse_customer_key", &"*** Sensitive Data Redacted ***");
        formatter.field("sse_customer_key_md5", &self.sse_customer_key_md5);
        formatter.field("sse_kms_key_id", &"*** Sensitive Data Redacted ***");
        formatter.field(
            "sse_kms_encryption_context",
            &"*** Sensitive Data Redacted ***",
        );
        formatter.field("bucket_key_enabled", &self.bucket_key_enabled);
        formatter.field("request_payer", &self.request_payer);
        formatter.field("tagging", &self.tagging);
        formatter.field("object_lock_mode", &self.object_lock_mode);
        formatter.field(
            "object_lock_retain_until_date",
            &self.object_lock_retain_until_date(),
        );
        formatter.field(
            "object_lock_legal_hold_status",
            &self.object_lock_legal_hold_status(),
        );
        formatter.field("expected_bucket_owner", &self.expected_bucket_owner);
        formatter.finish()
    }
}

/// A builder for [`UploadRequest`].
#[non_exhaustive]
#[derive(Default)]
pub struct UploadRequestBuilder {
    pub(crate) acl: Option<aws_sdk_s3::types::ObjectCannedAcl>,
    pub(crate) body: Option<crate::io::InputStream>,
    pub(crate) bucket: Option<String>,
    pub(crate) cache_control: Option<String>,
    pub(crate) content_disposition: Option<String>,
    pub(crate) content_encoding: Option<String>,
    pub(crate) content_language: Option<String>,
    pub(crate) content_length: Option<i64>,
    pub(crate) content_md5: Option<String>,
    pub(crate) content_type: Option<String>,
    pub(crate) checksum_algorithm: Option<aws_sdk_s3::types::ChecksumAlgorithm>,
    pub(crate) checksum_crc32: Option<String>,
    pub(crate) checksum_crc32_c: Option<String>,
    pub(crate) checksum_sha1: Option<String>,
    pub(crate) checksum_sha256: Option<String>,
    pub(crate) expires: Option<::aws_smithy_types::DateTime>,
    pub(crate) grant_full_control: Option<String>,
    pub(crate) grant_read: Option<String>,
    pub(crate) grant_read_acp: Option<String>,
    pub(crate) grant_write_acp: Option<String>,
    pub(crate) key: Option<String>,
    pub(crate) metadata: Option<::std::collections::HashMap<String, String>>,
    pub(crate) server_side_encryption: Option<aws_sdk_s3::types::ServerSideEncryption>,
    pub(crate) storage_class: Option<aws_sdk_s3::types::StorageClass>,
    pub(crate) website_redirect_location: Option<String>,
    pub(crate) sse_customer_algorithm: Option<String>,
    pub(crate) sse_customer_key: Option<String>,
    pub(crate) sse_customer_key_md5: Option<String>,
    pub(crate) sse_kms_key_id: Option<String>,
    pub(crate) sse_kms_encryption_context: Option<String>,
    pub(crate) bucket_key_enabled: Option<bool>,
    pub(crate) request_payer: Option<aws_sdk_s3::types::RequestPayer>,
    pub(crate) tagging: Option<String>,
    pub(crate) object_lock_mode: Option<aws_sdk_s3::types::ObjectLockMode>,
    pub(crate) object_lock_retain_until_date: Option<::aws_smithy_types::DateTime>,
    pub(crate) object_lock_legal_hold_status: Option<aws_sdk_s3::types::ObjectLockLegalHoldStatus>,
    pub(crate) expected_bucket_owner: Option<String>,
}

impl UploadRequestBuilder {
    /// <p>The canned ACL to apply to the object. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/acl-overview.html#CannedACL">Canned ACL</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p>When adding a new object, you can use headers to grant ACL-based permissions to individual Amazon Web Services accounts or to predefined groups defined by Amazon S3. These permissions are then added to the ACL on the object. By default, all objects are private. Only the owner has full access control. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/acl-overview.html">Access Control List (ACL) Overview</a> and <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/acl-using-rest-api.html">Managing ACLs Using the REST API</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p>If the bucket that you're uploading objects to uses the bucket owner enforced setting for S3 Object Ownership, ACLs are disabled and no longer affect permissions. Buckets that use this setting only accept PUT requests that don't specify an ACL or PUT requests that specify bucket owner full control ACLs, such as the <code>bucket-owner-full-control</code> canned ACL or an equivalent form of this ACL expressed in the XML format. PUT requests that contain other ACLs (for example, custom grants to certain Amazon Web Services accounts) fail and return a <code>400</code> error with the error code <code>AccessControlListNotSupported</code>. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/about-object-ownership.html"> Controlling ownership of objects and disabling ACLs</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <ul>
    /// <li>
    /// <p>This functionality is not supported for directory buckets.</p></li>
    /// <li>
    /// <p>This functionality is not supported for Amazon S3 on Outposts.</p></li>
    /// </ul>
    /// </note>
    pub fn acl(mut self, input: aws_sdk_s3::types::ObjectCannedAcl) -> Self {
        self.acl = Some(input);
        self
    }
    /// <p>The canned ACL to apply to the object. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/acl-overview.html#CannedACL">Canned ACL</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p>When adding a new object, you can use headers to grant ACL-based permissions to individual Amazon Web Services accounts or to predefined groups defined by Amazon S3. These permissions are then added to the ACL on the object. By default, all objects are private. Only the owner has full access control. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/acl-overview.html">Access Control List (ACL) Overview</a> and <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/acl-using-rest-api.html">Managing ACLs Using the REST API</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p>If the bucket that you're uploading objects to uses the bucket owner enforced setting for S3 Object Ownership, ACLs are disabled and no longer affect permissions. Buckets that use this setting only accept PUT requests that don't specify an ACL or PUT requests that specify bucket owner full control ACLs, such as the <code>bucket-owner-full-control</code> canned ACL or an equivalent form of this ACL expressed in the XML format. PUT requests that contain other ACLs (for example, custom grants to certain Amazon Web Services accounts) fail and return a <code>400</code> error with the error code <code>AccessControlListNotSupported</code>. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/about-object-ownership.html"> Controlling ownership of objects and disabling ACLs</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <ul>
    /// <li>
    /// <p>This functionality is not supported for directory buckets.</p></li>
    /// <li>
    /// <p>This functionality is not supported for Amazon S3 on Outposts.</p></li>
    /// </ul>
    /// </note>
    pub fn set_acl(mut self, input: Option<aws_sdk_s3::types::ObjectCannedAcl>) -> Self {
        self.acl = input;
        self
    }
    /// <p>The canned ACL to apply to the object. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/acl-overview.html#CannedACL">Canned ACL</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p>When adding a new object, you can use headers to grant ACL-based permissions to individual Amazon Web Services accounts or to predefined groups defined by Amazon S3. These permissions are then added to the ACL on the object. By default, all objects are private. Only the owner has full access control. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/acl-overview.html">Access Control List (ACL) Overview</a> and <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/acl-using-rest-api.html">Managing ACLs Using the REST API</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p>If the bucket that you're uploading objects to uses the bucket owner enforced setting for S3 Object Ownership, ACLs are disabled and no longer affect permissions. Buckets that use this setting only accept PUT requests that don't specify an ACL or PUT requests that specify bucket owner full control ACLs, such as the <code>bucket-owner-full-control</code> canned ACL or an equivalent form of this ACL expressed in the XML format. PUT requests that contain other ACLs (for example, custom grants to certain Amazon Web Services accounts) fail and return a <code>400</code> error with the error code <code>AccessControlListNotSupported</code>. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/about-object-ownership.html"> Controlling ownership of objects and disabling ACLs</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <ul>
    /// <li>
    /// <p>This functionality is not supported for directory buckets.</p></li>
    /// <li>
    /// <p>This functionality is not supported for Amazon S3 on Outposts.</p></li>
    /// </ul>
    /// </note>
    pub fn get_acl(&self) -> &Option<aws_sdk_s3::types::ObjectCannedAcl> {
        &self.acl
    }
    /// <p>Object data.</p>
    pub fn body(mut self, input: crate::io::InputStream) -> Self {
        self.body = Some(input);
        self
    }
    /// <p>Object data.</p>
    pub fn set_body(mut self, input: Option<crate::io::InputStream>) -> Self {
        self.body = input;
        self
    }

    /// <p>Object data.</p>
    pub fn get_body(&self) -> &Option<crate::io::InputStream> {
        &self.body
    }

    /// <p>The bucket name to which the PUT action was initiated.</p>
    /// <p><b>Directory buckets</b> - When you use this operation with a directory bucket, you must use virtual-hosted-style requests in the format <code> <i>Bucket_name</i>.s3express-<i>az_id</i>.<i>region</i>.amazonaws.com</code>. Path-style requests are not supported. Directory bucket names must be unique in the chosen Availability Zone. Bucket names must follow the format <code> <i>bucket_base_name</i>--<i>az-id</i>--x-s3</code> (for example, <code> <i>DOC-EXAMPLE-BUCKET</i>--<i>usw2-az1</i>--x-s3</code>). For information about bucket naming restrictions, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/directory-bucket-naming-rules.html">Directory bucket naming rules</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p><b>Access points</b> - When you use this action with an access point, you must provide the alias of the access point in place of the bucket name or specify the access point ARN. When using the access point ARN, you must direct requests to the access point hostname. The access point hostname takes the form <i>AccessPointName</i>-<i>AccountId</i>.s3-accesspoint.<i>Region</i>.amazonaws.com. When using this action with an access point through the Amazon Web Services SDKs, you provide the access point ARN in place of the bucket name. For more information about access point ARNs, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/using-access-points.html">Using access points</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <p>Access points and Object Lambda access points are not supported by directory buckets.</p>
    /// </note>
    /// <p><b>S3 on Outposts</b> - When you use this action with Amazon S3 on Outposts, you must direct requests to the S3 on Outposts hostname. The S3 on Outposts hostname takes the form <code> <i>AccessPointName</i>-<i>AccountId</i>.<i>outpostID</i>.s3-outposts.<i>Region</i>.amazonaws.com</code>. When you use this action with S3 on Outposts through the Amazon Web Services SDKs, you provide the Outposts access point ARN in place of the bucket name. For more information about S3 on Outposts ARNs, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/S3onOutposts.html">What is S3 on Outposts?</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// This field is required.
    pub fn bucket(mut self, input: impl Into<String>) -> Self {
        self.bucket = Some(input.into());
        self
    }
    /// <p>The bucket name to which the PUT action was initiated.</p>
    /// <p><b>Directory buckets</b> - When you use this operation with a directory bucket, you must use virtual-hosted-style requests in the format <code> <i>Bucket_name</i>.s3express-<i>az_id</i>.<i>region</i>.amazonaws.com</code>. Path-style requests are not supported. Directory bucket names must be unique in the chosen Availability Zone. Bucket names must follow the format <code> <i>bucket_base_name</i>--<i>az-id</i>--x-s3</code> (for example, <code> <i>DOC-EXAMPLE-BUCKET</i>--<i>usw2-az1</i>--x-s3</code>). For information about bucket naming restrictions, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/directory-bucket-naming-rules.html">Directory bucket naming rules</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p><b>Access points</b> - When you use this action with an access point, you must provide the alias of the access point in place of the bucket name or specify the access point ARN. When using the access point ARN, you must direct requests to the access point hostname. The access point hostname takes the form <i>AccessPointName</i>-<i>AccountId</i>.s3-accesspoint.<i>Region</i>.amazonaws.com. When using this action with an access point through the Amazon Web Services SDKs, you provide the access point ARN in place of the bucket name. For more information about access point ARNs, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/using-access-points.html">Using access points</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <p>Access points and Object Lambda access points are not supported by directory buckets.</p>
    /// </note>
    /// <p><b>S3 on Outposts</b> - When you use this action with Amazon S3 on Outposts, you must direct requests to the S3 on Outposts hostname. The S3 on Outposts hostname takes the form <code> <i>AccessPointName</i>-<i>AccountId</i>.<i>outpostID</i>.s3-outposts.<i>Region</i>.amazonaws.com</code>. When you use this action with S3 on Outposts through the Amazon Web Services SDKs, you provide the Outposts access point ARN in place of the bucket name. For more information about S3 on Outposts ARNs, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/S3onOutposts.html">What is S3 on Outposts?</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn set_bucket(mut self, input: Option<String>) -> Self {
        self.bucket = input;
        self
    }
    /// <p>The bucket name to which the PUT action was initiated.</p>
    /// <p><b>Directory buckets</b> - When you use this operation with a directory bucket, you must use virtual-hosted-style requests in the format <code> <i>Bucket_name</i>.s3express-<i>az_id</i>.<i>region</i>.amazonaws.com</code>. Path-style requests are not supported. Directory bucket names must be unique in the chosen Availability Zone. Bucket names must follow the format <code> <i>bucket_base_name</i>--<i>az-id</i>--x-s3</code> (for example, <code> <i>DOC-EXAMPLE-BUCKET</i>--<i>usw2-az1</i>--x-s3</code>). For information about bucket naming restrictions, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/directory-bucket-naming-rules.html">Directory bucket naming rules</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p><b>Access points</b> - When you use this action with an access point, you must provide the alias of the access point in place of the bucket name or specify the access point ARN. When using the access point ARN, you must direct requests to the access point hostname. The access point hostname takes the form <i>AccessPointName</i>-<i>AccountId</i>.s3-accesspoint.<i>Region</i>.amazonaws.com. When using this action with an access point through the Amazon Web Services SDKs, you provide the access point ARN in place of the bucket name. For more information about access point ARNs, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/using-access-points.html">Using access points</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <p>Access points and Object Lambda access points are not supported by directory buckets.</p>
    /// </note>
    /// <p><b>S3 on Outposts</b> - When you use this action with Amazon S3 on Outposts, you must direct requests to the S3 on Outposts hostname. The S3 on Outposts hostname takes the form <code> <i>AccessPointName</i>-<i>AccountId</i>.<i>outpostID</i>.s3-outposts.<i>Region</i>.amazonaws.com</code>. When you use this action with S3 on Outposts through the Amazon Web Services SDKs, you provide the Outposts access point ARN in place of the bucket name. For more information about S3 on Outposts ARNs, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/S3onOutposts.html">What is S3 on Outposts?</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn get_bucket(&self) -> &Option<String> {
        &self.bucket
    }
    /// <p>Can be used to specify caching behavior along the request/reply chain. For more information, see <a href="http://www.w3.org/Protocols/rfc2616/rfc2616-sec14.html#sec14.9">http://www.w3.org/Protocols/rfc2616/rfc2616-sec14.html#sec14.9</a>.</p>
    pub fn cache_control(mut self, input: impl Into<String>) -> Self {
        self.cache_control = Some(input.into());
        self
    }
    /// <p>Can be used to specify caching behavior along the request/reply chain. For more information, see <a href="http://www.w3.org/Protocols/rfc2616/rfc2616-sec14.html#sec14.9">http://www.w3.org/Protocols/rfc2616/rfc2616-sec14.html#sec14.9</a>.</p>
    pub fn set_cache_control(mut self, input: Option<String>) -> Self {
        self.cache_control = input;
        self
    }
    /// <p>Can be used to specify caching behavior along the request/reply chain. For more information, see <a href="http://www.w3.org/Protocols/rfc2616/rfc2616-sec14.html#sec14.9">http://www.w3.org/Protocols/rfc2616/rfc2616-sec14.html#sec14.9</a>.</p>
    pub fn get_cache_control(&self) -> &Option<String> {
        &self.cache_control
    }
    /// <p>Specifies presentational information for the object. For more information, see <a href="https://www.rfc-editor.org/rfc/rfc6266#section-4">https://www.rfc-editor.org/rfc/rfc6266#section-4</a>.</p>
    pub fn content_disposition(mut self, input: impl Into<String>) -> Self {
        self.content_disposition = Some(input.into());
        self
    }
    /// <p>Specifies presentational information for the object. For more information, see <a href="https://www.rfc-editor.org/rfc/rfc6266#section-4">https://www.rfc-editor.org/rfc/rfc6266#section-4</a>.</p>
    pub fn set_content_disposition(mut self, input: Option<String>) -> Self {
        self.content_disposition = input;
        self
    }
    /// <p>Specifies presentational information for the object. For more information, see <a href="https://www.rfc-editor.org/rfc/rfc6266#section-4">https://www.rfc-editor.org/rfc/rfc6266#section-4</a>.</p>
    pub fn get_content_disposition(&self) -> &Option<String> {
        &self.content_disposition
    }
    /// <p>Specifies what content encodings have been applied to the object and thus what decoding mechanisms must be applied to obtain the media-type referenced by the Content-Type header field. For more information, see <a href="https://www.rfc-editor.org/rfc/rfc9110.html#field.content-encoding">https://www.rfc-editor.org/rfc/rfc9110.html#field.content-encoding</a>.</p>
    pub fn content_encoding(mut self, input: impl Into<String>) -> Self {
        self.content_encoding = Some(input.into());
        self
    }
    /// <p>Specifies what content encodings have been applied to the object and thus what decoding mechanisms must be applied to obtain the media-type referenced by the Content-Type header field. For more information, see <a href="https://www.rfc-editor.org/rfc/rfc9110.html#field.content-encoding">https://www.rfc-editor.org/rfc/rfc9110.html#field.content-encoding</a>.</p>
    pub fn set_content_encoding(mut self, input: Option<String>) -> Self {
        self.content_encoding = input;
        self
    }
    /// <p>Specifies what content encodings have been applied to the object and thus what decoding mechanisms must be applied to obtain the media-type referenced by the Content-Type header field. For more information, see <a href="https://www.rfc-editor.org/rfc/rfc9110.html#field.content-encoding">https://www.rfc-editor.org/rfc/rfc9110.html#field.content-encoding</a>.</p>
    pub fn get_content_encoding(&self) -> &Option<String> {
        &self.content_encoding
    }
    /// <p>The language the content is in.</p>
    pub fn content_language(mut self, input: impl Into<String>) -> Self {
        self.content_language = Some(input.into());
        self
    }
    /// <p>The language the content is in.</p>
    pub fn set_content_language(mut self, input: Option<String>) -> Self {
        self.content_language = input;
        self
    }
    /// <p>The language the content is in.</p>
    pub fn get_content_language(&self) -> &Option<String> {
        &self.content_language
    }
    /// <p>Size of the body in bytes. This parameter is useful when the size of the body cannot be determined automatically. For more information, see <a href="https://www.rfc-editor.org/rfc/rfc9110.html#name-content-length">https://www.rfc-editor.org/rfc/rfc9110.html#name-content-length</a>.</p>
    pub fn content_length(mut self, input: i64) -> Self {
        self.content_length = Some(input);
        self
    }
    /// <p>Size of the body in bytes. This parameter is useful when the size of the body cannot be determined automatically. For more information, see <a href="https://www.rfc-editor.org/rfc/rfc9110.html#name-content-length">https://www.rfc-editor.org/rfc/rfc9110.html#name-content-length</a>.</p>
    pub fn set_content_length(mut self, input: Option<i64>) -> Self {
        self.content_length = input;
        self
    }
    /// <p>Size of the body in bytes. This parameter is useful when the size of the body cannot be determined automatically. For more information, see <a href="https://www.rfc-editor.org/rfc/rfc9110.html#name-content-length">https://www.rfc-editor.org/rfc/rfc9110.html#name-content-length</a>.</p>
    pub fn get_content_length(&self) -> &Option<i64> {
        &self.content_length
    }
    /// <p>The base64-encoded 128-bit MD5 digest of the message (without the headers) according to RFC 1864. This header can be used as a message integrity check to verify that the data is the same data that was originally sent. Although it is optional, we recommend using the Content-MD5 mechanism as an end-to-end integrity check. For more information about REST request authentication, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/RESTAuthentication.html">REST Authentication</a>.</p><note>
    /// <p>The <code>Content-MD5</code> header is required for any request to upload an object with a retention period configured using Amazon S3 Object Lock. For more information about Amazon S3 Object Lock, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/object-lock-overview.html">Amazon S3 Object Lock Overview</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// </note> <note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn content_md5(mut self, input: impl Into<String>) -> Self {
        self.content_md5 = Some(input.into());
        self
    }
    /// <p>The base64-encoded 128-bit MD5 digest of the message (without the headers) according to RFC 1864. This header can be used as a message integrity check to verify that the data is the same data that was originally sent. Although it is optional, we recommend using the Content-MD5 mechanism as an end-to-end integrity check. For more information about REST request authentication, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/RESTAuthentication.html">REST Authentication</a>.</p><note>
    /// <p>The <code>Content-MD5</code> header is required for any request to upload an object with a retention period configured using Amazon S3 Object Lock. For more information about Amazon S3 Object Lock, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/object-lock-overview.html">Amazon S3 Object Lock Overview</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// </note> <note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn set_content_md5(mut self, input: Option<String>) -> Self {
        self.content_md5 = input;
        self
    }
    /// <p>The base64-encoded 128-bit MD5 digest of the message (without the headers) according to RFC 1864. This header can be used as a message integrity check to verify that the data is the same data that was originally sent. Although it is optional, we recommend using the Content-MD5 mechanism as an end-to-end integrity check. For more information about REST request authentication, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/RESTAuthentication.html">REST Authentication</a>.</p><note>
    /// <p>The <code>Content-MD5</code> header is required for any request to upload an object with a retention period configured using Amazon S3 Object Lock. For more information about Amazon S3 Object Lock, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/object-lock-overview.html">Amazon S3 Object Lock Overview</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// </note> <note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn get_content_md5(&self) -> &Option<String> {
        &self.content_md5
    }
    /// <p>A standard MIME type describing the format of the contents. For more information, see <a href="https://www.rfc-editor.org/rfc/rfc9110.html#name-content-type">https://www.rfc-editor.org/rfc/rfc9110.html#name-content-type</a>.</p>
    pub fn content_type(mut self, input: impl Into<String>) -> Self {
        self.content_type = Some(input.into());
        self
    }
    /// <p>A standard MIME type describing the format of the contents. For more information, see <a href="https://www.rfc-editor.org/rfc/rfc9110.html#name-content-type">https://www.rfc-editor.org/rfc/rfc9110.html#name-content-type</a>.</p>
    pub fn set_content_type(mut self, input: Option<String>) -> Self {
        self.content_type = input;
        self
    }
    /// <p>A standard MIME type describing the format of the contents. For more information, see <a href="https://www.rfc-editor.org/rfc/rfc9110.html#name-content-type">https://www.rfc-editor.org/rfc/rfc9110.html#name-content-type</a>.</p>
    pub fn get_content_type(&self) -> &Option<String> {
        &self.content_type
    }
    /// <p>Indicates the algorithm used to create the checksum for the object when you use the SDK. This header will not provide any additional functionality if you don't use the SDK. When you send this header, there must be a corresponding <code>x-amz-checksum-<i>algorithm</i> </code> or <code>x-amz-trailer</code> header sent. Otherwise, Amazon S3 fails the request with the HTTP status code <code>400 Bad Request</code>.</p>
    /// <p>For the <code>x-amz-checksum-<i>algorithm</i> </code> header, replace <code> <i>algorithm</i> </code> with the supported algorithm from the following list:</p>
    /// <ul>
    /// <li>
    /// <p>CRC32</p></li>
    /// <li>
    /// <p>CRC32C</p></li>
    /// <li>
    /// <p>SHA1</p></li>
    /// <li>
    /// <p>SHA256</p></li>
    /// </ul>
    /// <p>For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html">Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p>If the individual checksum value you provide through <code>x-amz-checksum-<i>algorithm</i> </code> doesn't match the checksum algorithm you set through <code>x-amz-sdk-checksum-algorithm</code>, Amazon S3 ignores any provided <code>ChecksumAlgorithm</code> parameter and uses the checksum algorithm that matches the provided value in <code>x-amz-checksum-<i>algorithm</i> </code>.</p><note>
    /// <p>For directory buckets, when you use Amazon Web Services SDKs, <code>CRC32</code> is the default checksum algorithm that's used for performance.</p>
    /// </note>
    pub fn checksum_algorithm(mut self, input: aws_sdk_s3::types::ChecksumAlgorithm) -> Self {
        self.checksum_algorithm = Some(input);
        self
    }
    /// <p>Indicates the algorithm used to create the checksum for the object when you use the SDK. This header will not provide any additional functionality if you don't use the SDK. When you send this header, there must be a corresponding <code>x-amz-checksum-<i>algorithm</i> </code> or <code>x-amz-trailer</code> header sent. Otherwise, Amazon S3 fails the request with the HTTP status code <code>400 Bad Request</code>.</p>
    /// <p>For the <code>x-amz-checksum-<i>algorithm</i> </code> header, replace <code> <i>algorithm</i> </code> with the supported algorithm from the following list:</p>
    /// <ul>
    /// <li>
    /// <p>CRC32</p></li>
    /// <li>
    /// <p>CRC32C</p></li>
    /// <li>
    /// <p>SHA1</p></li>
    /// <li>
    /// <p>SHA256</p></li>
    /// </ul>
    /// <p>For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html">Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p>If the individual checksum value you provide through <code>x-amz-checksum-<i>algorithm</i> </code> doesn't match the checksum algorithm you set through <code>x-amz-sdk-checksum-algorithm</code>, Amazon S3 ignores any provided <code>ChecksumAlgorithm</code> parameter and uses the checksum algorithm that matches the provided value in <code>x-amz-checksum-<i>algorithm</i> </code>.</p><note>
    /// <p>For directory buckets, when you use Amazon Web Services SDKs, <code>CRC32</code> is the default checksum algorithm that's used for performance.</p>
    /// </note>
    pub fn set_checksum_algorithm(
        mut self,
        input: Option<aws_sdk_s3::types::ChecksumAlgorithm>,
    ) -> Self {
        self.checksum_algorithm = input;
        self
    }
    /// <p>Indicates the algorithm used to create the checksum for the object when you use the SDK. This header will not provide any additional functionality if you don't use the SDK. When you send this header, there must be a corresponding <code>x-amz-checksum-<i>algorithm</i> </code> or <code>x-amz-trailer</code> header sent. Otherwise, Amazon S3 fails the request with the HTTP status code <code>400 Bad Request</code>.</p>
    /// <p>For the <code>x-amz-checksum-<i>algorithm</i> </code> header, replace <code> <i>algorithm</i> </code> with the supported algorithm from the following list:</p>
    /// <ul>
    /// <li>
    /// <p>CRC32</p></li>
    /// <li>
    /// <p>CRC32C</p></li>
    /// <li>
    /// <p>SHA1</p></li>
    /// <li>
    /// <p>SHA256</p></li>
    /// </ul>
    /// <p>For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html">Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p>If the individual checksum value you provide through <code>x-amz-checksum-<i>algorithm</i> </code> doesn't match the checksum algorithm you set through <code>x-amz-sdk-checksum-algorithm</code>, Amazon S3 ignores any provided <code>ChecksumAlgorithm</code> parameter and uses the checksum algorithm that matches the provided value in <code>x-amz-checksum-<i>algorithm</i> </code>.</p><note>
    /// <p>For directory buckets, when you use Amazon Web Services SDKs, <code>CRC32</code> is the default checksum algorithm that's used for performance.</p>
    /// </note>
    pub fn get_checksum_algorithm(&self) -> &Option<aws_sdk_s3::types::ChecksumAlgorithm> {
        &self.checksum_algorithm
    }
    /// <p>This header can be used as a data integrity check to verify that the data received is the same data that was originally sent. This header specifies the base64-encoded, 32-bit CRC32 checksum of the object. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html">Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn checksum_crc32(mut self, input: impl Into<String>) -> Self {
        self.checksum_crc32 = Some(input.into());
        self
    }
    /// <p>This header can be used as a data integrity check to verify that the data received is the same data that was originally sent. This header specifies the base64-encoded, 32-bit CRC32 checksum of the object. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html">Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn set_checksum_crc32(mut self, input: Option<String>) -> Self {
        self.checksum_crc32 = input;
        self
    }
    /// <p>This header can be used as a data integrity check to verify that the data received is the same data that was originally sent. This header specifies the base64-encoded, 32-bit CRC32 checksum of the object. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html">Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn get_checksum_crc32(&self) -> &Option<String> {
        &self.checksum_crc32
    }
    /// <p>This header can be used as a data integrity check to verify that the data received is the same data that was originally sent. This header specifies the base64-encoded, 32-bit CRC32C checksum of the object. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html">Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn checksum_crc32_c(mut self, input: impl Into<String>) -> Self {
        self.checksum_crc32_c = Some(input.into());
        self
    }
    /// <p>This header can be used as a data integrity check to verify that the data received is the same data that was originally sent. This header specifies the base64-encoded, 32-bit CRC32C checksum of the object. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html">Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn set_checksum_crc32_c(mut self, input: Option<String>) -> Self {
        self.checksum_crc32_c = input;
        self
    }
    /// <p>This header can be used as a data integrity check to verify that the data received is the same data that was originally sent. This header specifies the base64-encoded, 32-bit CRC32C checksum of the object. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html">Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn get_checksum_crc32_c(&self) -> &Option<String> {
        &self.checksum_crc32_c
    }
    /// <p>This header can be used as a data integrity check to verify that the data received is the same data that was originally sent. This header specifies the base64-encoded, 160-bit SHA-1 digest of the object. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html">Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn checksum_sha1(mut self, input: impl Into<String>) -> Self {
        self.checksum_sha1 = Some(input.into());
        self
    }
    /// <p>This header can be used as a data integrity check to verify that the data received is the same data that was originally sent. This header specifies the base64-encoded, 160-bit SHA-1 digest of the object. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html">Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn set_checksum_sha1(mut self, input: Option<String>) -> Self {
        self.checksum_sha1 = input;
        self
    }
    /// <p>This header can be used as a data integrity check to verify that the data received is the same data that was originally sent. This header specifies the base64-encoded, 160-bit SHA-1 digest of the object. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html">Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn get_checksum_sha1(&self) -> &Option<String> {
        &self.checksum_sha1
    }
    /// <p>This header can be used as a data integrity check to verify that the data received is the same data that was originally sent. This header specifies the base64-encoded, 256-bit SHA-256 digest of the object. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html">Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn checksum_sha256(mut self, input: impl Into<String>) -> Self {
        self.checksum_sha256 = Some(input.into());
        self
    }
    /// <p>This header can be used as a data integrity check to verify that the data received is the same data that was originally sent. This header specifies the base64-encoded, 256-bit SHA-256 digest of the object. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html">Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn set_checksum_sha256(mut self, input: Option<String>) -> Self {
        self.checksum_sha256 = input;
        self
    }
    /// <p>This header can be used as a data integrity check to verify that the data received is the same data that was originally sent. This header specifies the base64-encoded, 256-bit SHA-256 digest of the object. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html">Checking object integrity</a> in the <i>Amazon S3 User Guide</i>.</p>
    pub fn get_checksum_sha256(&self) -> &Option<String> {
        &self.checksum_sha256
    }
    /// <p>The date and time at which the object is no longer cacheable. For more information, see <a href="https://www.rfc-editor.org/rfc/rfc7234#section-5.3">https://www.rfc-editor.org/rfc/rfc7234#section-5.3</a>.</p>
    pub fn expires(mut self, input: ::aws_smithy_types::DateTime) -> Self {
        self.expires = Some(input);
        self
    }
    /// <p>The date and time at which the object is no longer cacheable. For more information, see <a href="https://www.rfc-editor.org/rfc/rfc7234#section-5.3">https://www.rfc-editor.org/rfc/rfc7234#section-5.3</a>.</p>
    pub fn set_expires(mut self, input: Option<::aws_smithy_types::DateTime>) -> Self {
        self.expires = input;
        self
    }
    /// <p>The date and time at which the object is no longer cacheable. For more information, see <a href="https://www.rfc-editor.org/rfc/rfc7234#section-5.3">https://www.rfc-editor.org/rfc/rfc7234#section-5.3</a>.</p>
    pub fn get_expires(&self) -> &Option<::aws_smithy_types::DateTime> {
        &self.expires
    }
    /// <p>Gives the grantee READ, READ_ACP, and WRITE_ACP permissions on the object.</p><note>
    /// <ul>
    /// <li>
    /// <p>This functionality is not supported for directory buckets.</p></li>
    /// <li>
    /// <p>This functionality is not supported for Amazon S3 on Outposts.</p></li>
    /// </ul>
    /// </note>
    pub fn grant_full_control(mut self, input: impl Into<String>) -> Self {
        self.grant_full_control = Some(input.into());
        self
    }
    /// <p>Gives the grantee READ, READ_ACP, and WRITE_ACP permissions on the object.</p><note>
    /// <ul>
    /// <li>
    /// <p>This functionality is not supported for directory buckets.</p></li>
    /// <li>
    /// <p>This functionality is not supported for Amazon S3 on Outposts.</p></li>
    /// </ul>
    /// </note>
    pub fn set_grant_full_control(mut self, input: Option<String>) -> Self {
        self.grant_full_control = input;
        self
    }
    /// <p>Gives the grantee READ, READ_ACP, and WRITE_ACP permissions on the object.</p><note>
    /// <ul>
    /// <li>
    /// <p>This functionality is not supported for directory buckets.</p></li>
    /// <li>
    /// <p>This functionality is not supported for Amazon S3 on Outposts.</p></li>
    /// </ul>
    /// </note>
    pub fn get_grant_full_control(&self) -> &Option<String> {
        &self.grant_full_control
    }
    /// <p>Allows grantee to read the object data and its metadata.</p><note>
    /// <ul>
    /// <li>
    /// <p>This functionality is not supported for directory buckets.</p></li>
    /// <li>
    /// <p>This functionality is not supported for Amazon S3 on Outposts.</p></li>
    /// </ul>
    /// </note>
    pub fn grant_read(mut self, input: impl Into<String>) -> Self {
        self.grant_read = Some(input.into());
        self
    }
    /// <p>Allows grantee to read the object data and its metadata.</p><note>
    /// <ul>
    /// <li>
    /// <p>This functionality is not supported for directory buckets.</p></li>
    /// <li>
    /// <p>This functionality is not supported for Amazon S3 on Outposts.</p></li>
    /// </ul>
    /// </note>
    pub fn set_grant_read(mut self, input: Option<String>) -> Self {
        self.grant_read = input;
        self
    }
    /// <p>Allows grantee to read the object data and its metadata.</p><note>
    /// <ul>
    /// <li>
    /// <p>This functionality is not supported for directory buckets.</p></li>
    /// <li>
    /// <p>This functionality is not supported for Amazon S3 on Outposts.</p></li>
    /// </ul>
    /// </note>
    pub fn get_grant_read(&self) -> &Option<String> {
        &self.grant_read
    }
    /// <p>Allows grantee to read the object ACL.</p><note>
    /// <ul>
    /// <li>
    /// <p>This functionality is not supported for directory buckets.</p></li>
    /// <li>
    /// <p>This functionality is not supported for Amazon S3 on Outposts.</p></li>
    /// </ul>
    /// </note>
    pub fn grant_read_acp(mut self, input: impl Into<String>) -> Self {
        self.grant_read_acp = Some(input.into());
        self
    }
    /// <p>Allows grantee to read the object ACL.</p><note>
    /// <ul>
    /// <li>
    /// <p>This functionality is not supported for directory buckets.</p></li>
    /// <li>
    /// <p>This functionality is not supported for Amazon S3 on Outposts.</p></li>
    /// </ul>
    /// </note>
    pub fn set_grant_read_acp(mut self, input: Option<String>) -> Self {
        self.grant_read_acp = input;
        self
    }
    /// <p>Allows grantee to read the object ACL.</p><note>
    /// <ul>
    /// <li>
    /// <p>This functionality is not supported for directory buckets.</p></li>
    /// <li>
    /// <p>This functionality is not supported for Amazon S3 on Outposts.</p></li>
    /// </ul>
    /// </note>
    pub fn get_grant_read_acp(&self) -> &Option<String> {
        &self.grant_read_acp
    }
    /// <p>Allows grantee to write the ACL for the applicable object.</p><note>
    /// <ul>
    /// <li>
    /// <p>This functionality is not supported for directory buckets.</p></li>
    /// <li>
    /// <p>This functionality is not supported for Amazon S3 on Outposts.</p></li>
    /// </ul>
    /// </note>
    pub fn grant_write_acp(mut self, input: impl Into<String>) -> Self {
        self.grant_write_acp = Some(input.into());
        self
    }
    /// <p>Allows grantee to write the ACL for the applicable object.</p><note>
    /// <ul>
    /// <li>
    /// <p>This functionality is not supported for directory buckets.</p></li>
    /// <li>
    /// <p>This functionality is not supported for Amazon S3 on Outposts.</p></li>
    /// </ul>
    /// </note>
    pub fn set_grant_write_acp(mut self, input: Option<String>) -> Self {
        self.grant_write_acp = input;
        self
    }
    /// <p>Allows grantee to write the ACL for the applicable object.</p><note>
    /// <ul>
    /// <li>
    /// <p>This functionality is not supported for directory buckets.</p></li>
    /// <li>
    /// <p>This functionality is not supported for Amazon S3 on Outposts.</p></li>
    /// </ul>
    /// </note>
    pub fn get_grant_write_acp(&self) -> &Option<String> {
        &self.grant_write_acp
    }
    /// <p>Object key for which the PUT action was initiated.</p>
    /// This field is required.
    pub fn key(mut self, input: impl Into<String>) -> Self {
        self.key = Some(input.into());
        self
    }
    /// <p>Object key for which the PUT action was initiated.</p>
    pub fn set_key(mut self, input: Option<String>) -> Self {
        self.key = input;
        self
    }
    /// <p>Object key for which the PUT action was initiated.</p>
    pub fn get_key(&self) -> &Option<String> {
        &self.key
    }
    /// Adds a key-value pair to `metadata`.
    ///
    /// To override the contents of this collection use [`set_metadata`](Self::set_metadata).
    ///
    /// <p>A map of metadata to store with the object in S3.</p>
    pub fn metadata(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        let mut hash_map = self.metadata.unwrap_or_default();
        hash_map.insert(k.into(), v.into());
        self.metadata = Some(hash_map);
        self
    }
    /// <p>A map of metadata to store with the object in S3.</p>
    pub fn set_metadata(
        mut self,
        input: Option<::std::collections::HashMap<String, String>>,
    ) -> Self {
        self.metadata = input;
        self
    }
    /// <p>A map of metadata to store with the object in S3.</p>
    pub fn get_metadata(&self) -> &Option<::std::collections::HashMap<String, String>> {
        &self.metadata
    }
    /// <p>The server-side encryption algorithm that was used when you store this object in Amazon S3 (for example, <code>AES256</code>, <code>aws:kms</code>, <code>aws:kms:dsse</code>).</p>
    /// <p><b>General purpose buckets </b> - You have four mutually exclusive options to protect data using server-side encryption in Amazon S3, depending on how you choose to manage the encryption keys. Specifically, the encryption key options are Amazon S3 managed keys (SSE-S3), Amazon Web Services KMS keys (SSE-KMS or DSSE-KMS), and customer-provided keys (SSE-C). Amazon S3 encrypts data with server-side encryption by using Amazon S3 managed keys (SSE-S3) by default. You can optionally tell Amazon S3 to encrypt data at rest by using server-side encryption with other key options. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/UsingServerSideEncryption.html">Using Server-Side Encryption</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p><b>Directory buckets </b> - For directory buckets, only the server-side encryption with Amazon S3 managed keys (SSE-S3) (<code>AES256</code>) value is supported.</p>
    pub fn server_side_encryption(
        mut self,
        input: aws_sdk_s3::types::ServerSideEncryption,
    ) -> Self {
        self.server_side_encryption = Some(input);
        self
    }
    /// <p>The server-side encryption algorithm that was used when you store this object in Amazon S3 (for example, <code>AES256</code>, <code>aws:kms</code>, <code>aws:kms:dsse</code>).</p>
    /// <p><b>General purpose buckets </b> - You have four mutually exclusive options to protect data using server-side encryption in Amazon S3, depending on how you choose to manage the encryption keys. Specifically, the encryption key options are Amazon S3 managed keys (SSE-S3), Amazon Web Services KMS keys (SSE-KMS or DSSE-KMS), and customer-provided keys (SSE-C). Amazon S3 encrypts data with server-side encryption by using Amazon S3 managed keys (SSE-S3) by default. You can optionally tell Amazon S3 to encrypt data at rest by using server-side encryption with other key options. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/UsingServerSideEncryption.html">Using Server-Side Encryption</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p><b>Directory buckets </b> - For directory buckets, only the server-side encryption with Amazon S3 managed keys (SSE-S3) (<code>AES256</code>) value is supported.</p>
    pub fn set_server_side_encryption(
        mut self,
        input: Option<aws_sdk_s3::types::ServerSideEncryption>,
    ) -> Self {
        self.server_side_encryption = input;
        self
    }
    /// <p>The server-side encryption algorithm that was used when you store this object in Amazon S3 (for example, <code>AES256</code>, <code>aws:kms</code>, <code>aws:kms:dsse</code>).</p>
    /// <p><b>General purpose buckets </b> - You have four mutually exclusive options to protect data using server-side encryption in Amazon S3, depending on how you choose to manage the encryption keys. Specifically, the encryption key options are Amazon S3 managed keys (SSE-S3), Amazon Web Services KMS keys (SSE-KMS or DSSE-KMS), and customer-provided keys (SSE-C). Amazon S3 encrypts data with server-side encryption by using Amazon S3 managed keys (SSE-S3) by default. You can optionally tell Amazon S3 to encrypt data at rest by using server-side encryption with other key options. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/UsingServerSideEncryption.html">Using Server-Side Encryption</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p><b>Directory buckets </b> - For directory buckets, only the server-side encryption with Amazon S3 managed keys (SSE-S3) (<code>AES256</code>) value is supported.</p>
    pub fn get_server_side_encryption(&self) -> &Option<aws_sdk_s3::types::ServerSideEncryption> {
        &self.server_side_encryption
    }
    /// <p>By default, Amazon S3 uses the STANDARD Storage Class to store newly created objects. The STANDARD storage class provides high durability and high availability. Depending on performance needs, you can specify a different Storage Class. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/storage-class-intro.html">Storage Classes</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <ul>
    /// <li>
    /// <p>For directory buckets, only the S3 Express One Zone storage class is supported to store newly created objects.</p></li>
    /// <li>
    /// <p>Amazon S3 on Outposts only uses the OUTPOSTS Storage Class.</p></li>
    /// </ul>
    /// </note>
    pub fn storage_class(mut self, input: aws_sdk_s3::types::StorageClass) -> Self {
        self.storage_class = Some(input);
        self
    }
    /// <p>By default, Amazon S3 uses the STANDARD Storage Class to store newly created objects. The STANDARD storage class provides high durability and high availability. Depending on performance needs, you can specify a different Storage Class. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/storage-class-intro.html">Storage Classes</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <ul>
    /// <li>
    /// <p>For directory buckets, only the S3 Express One Zone storage class is supported to store newly created objects.</p></li>
    /// <li>
    /// <p>Amazon S3 on Outposts only uses the OUTPOSTS Storage Class.</p></li>
    /// </ul>
    /// </note>
    pub fn set_storage_class(mut self, input: Option<aws_sdk_s3::types::StorageClass>) -> Self {
        self.storage_class = input;
        self
    }
    /// <p>By default, Amazon S3 uses the STANDARD Storage Class to store newly created objects. The STANDARD storage class provides high durability and high availability. Depending on performance needs, you can specify a different Storage Class. For more information, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/storage-class-intro.html">Storage Classes</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <ul>
    /// <li>
    /// <p>For directory buckets, only the S3 Express One Zone storage class is supported to store newly created objects.</p></li>
    /// <li>
    /// <p>Amazon S3 on Outposts only uses the OUTPOSTS Storage Class.</p></li>
    /// </ul>
    /// </note>
    pub fn get_storage_class(&self) -> &Option<aws_sdk_s3::types::StorageClass> {
        &self.storage_class
    }
    /// <p>If the bucket is configured as a website, redirects requests for this object to another object in the same bucket or to an external URL. Amazon S3 stores the value of this header in the object metadata. For information about object metadata, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/UsingMetadata.html">Object Key and Metadata</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p>In the following example, the request header sets the redirect to an object (anotherPage.html) in the same bucket:</p>
    /// <p><code>x-amz-website-redirect-location: /anotherPage.html</code></p>
    /// <p>In the following example, the request header sets the object redirect to another website:</p>
    /// <p><code>x-amz-website-redirect-location: http://www.example.com/</code></p>
    /// <p>For more information about website hosting in Amazon S3, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/WebsiteHosting.html">Hosting Websites on Amazon S3</a> and <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/how-to-page-redirect.html">How to Configure Website Page Redirects</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn website_redirect_location(mut self, input: impl Into<String>) -> Self {
        self.website_redirect_location = Some(input.into());
        self
    }
    /// <p>If the bucket is configured as a website, redirects requests for this object to another object in the same bucket or to an external URL. Amazon S3 stores the value of this header in the object metadata. For information about object metadata, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/UsingMetadata.html">Object Key and Metadata</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p>In the following example, the request header sets the redirect to an object (anotherPage.html) in the same bucket:</p>
    /// <p><code>x-amz-website-redirect-location: /anotherPage.html</code></p>
    /// <p>In the following example, the request header sets the object redirect to another website:</p>
    /// <p><code>x-amz-website-redirect-location: http://www.example.com/</code></p>
    /// <p>For more information about website hosting in Amazon S3, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/WebsiteHosting.html">Hosting Websites on Amazon S3</a> and <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/how-to-page-redirect.html">How to Configure Website Page Redirects</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn set_website_redirect_location(mut self, input: Option<String>) -> Self {
        self.website_redirect_location = input;
        self
    }
    /// <p>If the bucket is configured as a website, redirects requests for this object to another object in the same bucket or to an external URL. Amazon S3 stores the value of this header in the object metadata. For information about object metadata, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/UsingMetadata.html">Object Key and Metadata</a> in the <i>Amazon S3 User Guide</i>.</p>
    /// <p>In the following example, the request header sets the redirect to an object (anotherPage.html) in the same bucket:</p>
    /// <p><code>x-amz-website-redirect-location: /anotherPage.html</code></p>
    /// <p>In the following example, the request header sets the object redirect to another website:</p>
    /// <p><code>x-amz-website-redirect-location: http://www.example.com/</code></p>
    /// <p>For more information about website hosting in Amazon S3, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/WebsiteHosting.html">Hosting Websites on Amazon S3</a> and <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/how-to-page-redirect.html">How to Configure Website Page Redirects</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn get_website_redirect_location(&self) -> &Option<String> {
        &self.website_redirect_location
    }
    /// <p>Specifies the algorithm to use when encrypting the object (for example, <code>AES256</code>).</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn sse_customer_algorithm(mut self, input: impl Into<String>) -> Self {
        self.sse_customer_algorithm = Some(input.into());
        self
    }
    /// <p>Specifies the algorithm to use when encrypting the object (for example, <code>AES256</code>).</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn set_sse_customer_algorithm(mut self, input: Option<String>) -> Self {
        self.sse_customer_algorithm = input;
        self
    }
    /// <p>Specifies the algorithm to use when encrypting the object (for example, <code>AES256</code>).</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn get_sse_customer_algorithm(&self) -> &Option<String> {
        &self.sse_customer_algorithm
    }
    /// <p>Specifies the customer-provided encryption key for Amazon S3 to use in encrypting data. This value is used to store the object and then it is discarded; Amazon S3 does not store the encryption key. The key must be appropriate for use with the algorithm specified in the <code>x-amz-server-side-encryption-customer-algorithm</code> header.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn sse_customer_key(mut self, input: impl Into<String>) -> Self {
        self.sse_customer_key = Some(input.into());
        self
    }
    /// <p>Specifies the customer-provided encryption key for Amazon S3 to use in encrypting data. This value is used to store the object and then it is discarded; Amazon S3 does not store the encryption key. The key must be appropriate for use with the algorithm specified in the <code>x-amz-server-side-encryption-customer-algorithm</code> header.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn set_sse_customer_key(mut self, input: Option<String>) -> Self {
        self.sse_customer_key = input;
        self
    }
    /// <p>Specifies the customer-provided encryption key for Amazon S3 to use in encrypting data. This value is used to store the object and then it is discarded; Amazon S3 does not store the encryption key. The key must be appropriate for use with the algorithm specified in the <code>x-amz-server-side-encryption-customer-algorithm</code> header.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn get_sse_customer_key(&self) -> &Option<String> {
        &self.sse_customer_key
    }
    /// <p>Specifies the 128-bit MD5 digest of the encryption key according to RFC 1321. Amazon S3 uses this header for a message integrity check to ensure that the encryption key was transmitted without error.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn sse_customer_key_md5(mut self, input: impl Into<String>) -> Self {
        self.sse_customer_key_md5 = Some(input.into());
        self
    }
    /// <p>Specifies the 128-bit MD5 digest of the encryption key according to RFC 1321. Amazon S3 uses this header for a message integrity check to ensure that the encryption key was transmitted without error.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn set_sse_customer_key_md5(mut self, input: Option<String>) -> Self {
        self.sse_customer_key_md5 = input;
        self
    }
    /// <p>Specifies the 128-bit MD5 digest of the encryption key according to RFC 1321. Amazon S3 uses this header for a message integrity check to ensure that the encryption key was transmitted without error.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn get_sse_customer_key_md5(&self) -> &Option<String> {
        &self.sse_customer_key_md5
    }
    /// <p>If <code>x-amz-server-side-encryption</code> has a valid value of <code>aws:kms</code> or <code>aws:kms:dsse</code>, this header specifies the ID (Key ID, Key ARN, or Key Alias) of the Key Management Service (KMS) symmetric encryption customer managed key that was used for the object. If you specify <code>x-amz-server-side-encryption:aws:kms</code> or <code>x-amz-server-side-encryption:aws:kms:dsse</code>, but do not provide<code> x-amz-server-side-encryption-aws-kms-key-id</code>, Amazon S3 uses the Amazon Web Services managed key (<code>aws/s3</code>) to protect the data. If the KMS key does not exist in the same account that's issuing the command, you must use the full ARN and not just the ID.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn sse_kms_key_id(mut self, input: impl Into<String>) -> Self {
        self.sse_kms_key_id = Some(input.into());
        self
    }
    /// <p>If <code>x-amz-server-side-encryption</code> has a valid value of <code>aws:kms</code> or <code>aws:kms:dsse</code>, this header specifies the ID (Key ID, Key ARN, or Key Alias) of the Key Management Service (KMS) symmetric encryption customer managed key that was used for the object. If you specify <code>x-amz-server-side-encryption:aws:kms</code> or <code>x-amz-server-side-encryption:aws:kms:dsse</code>, but do not provide<code> x-amz-server-side-encryption-aws-kms-key-id</code>, Amazon S3 uses the Amazon Web Services managed key (<code>aws/s3</code>) to protect the data. If the KMS key does not exist in the same account that's issuing the command, you must use the full ARN and not just the ID.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn set_sse_kms_key_id(mut self, input: Option<String>) -> Self {
        self.sse_kms_key_id = input;
        self
    }
    /// <p>If <code>x-amz-server-side-encryption</code> has a valid value of <code>aws:kms</code> or <code>aws:kms:dsse</code>, this header specifies the ID (Key ID, Key ARN, or Key Alias) of the Key Management Service (KMS) symmetric encryption customer managed key that was used for the object. If you specify <code>x-amz-server-side-encryption:aws:kms</code> or <code>x-amz-server-side-encryption:aws:kms:dsse</code>, but do not provide<code> x-amz-server-side-encryption-aws-kms-key-id</code>, Amazon S3 uses the Amazon Web Services managed key (<code>aws/s3</code>) to protect the data. If the KMS key does not exist in the same account that's issuing the command, you must use the full ARN and not just the ID.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn get_sse_kms_key_id(&self) -> &Option<String> {
        &self.sse_kms_key_id
    }
    /// <p>Specifies the Amazon Web Services KMS Encryption Context to use for object encryption. The value of this header is a base64-encoded UTF-8 string holding JSON with the encryption context key-value pairs. This value is stored as object metadata and automatically gets passed on to Amazon Web Services KMS for future <code>GetObject</code> or <code>CopyObject</code> operations on this object. This value must be explicitly added during <code>CopyObject</code> operations.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn sse_kms_encryption_context(mut self, input: impl Into<String>) -> Self {
        self.sse_kms_encryption_context = Some(input.into());
        self
    }
    /// <p>Specifies the Amazon Web Services KMS Encryption Context to use for object encryption. The value of this header is a base64-encoded UTF-8 string holding JSON with the encryption context key-value pairs. This value is stored as object metadata and automatically gets passed on to Amazon Web Services KMS for future <code>GetObject</code> or <code>CopyObject</code> operations on this object. This value must be explicitly added during <code>CopyObject</code> operations.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn set_sse_kms_encryption_context(mut self, input: Option<String>) -> Self {
        self.sse_kms_encryption_context = input;
        self
    }
    /// <p>Specifies the Amazon Web Services KMS Encryption Context to use for object encryption. The value of this header is a base64-encoded UTF-8 string holding JSON with the encryption context key-value pairs. This value is stored as object metadata and automatically gets passed on to Amazon Web Services KMS for future <code>GetObject</code> or <code>CopyObject</code> operations on this object. This value must be explicitly added during <code>CopyObject</code> operations.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn get_sse_kms_encryption_context(&self) -> &Option<String> {
        &self.sse_kms_encryption_context
    }
    /// <p>Specifies whether Amazon S3 should use an S3 Bucket Key for object encryption with server-side encryption using Key Management Service (KMS) keys (SSE-KMS). Setting this header to <code>true</code> causes Amazon S3 to use an S3 Bucket Key for object encryption with SSE-KMS.</p>
    /// <p>Specifying this header with a PUT action doesn’t affect bucket-level settings for S3 Bucket Key.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn bucket_key_enabled(mut self, input: bool) -> Self {
        self.bucket_key_enabled = Some(input);
        self
    }
    /// <p>Specifies whether Amazon S3 should use an S3 Bucket Key for object encryption with server-side encryption using Key Management Service (KMS) keys (SSE-KMS). Setting this header to <code>true</code> causes Amazon S3 to use an S3 Bucket Key for object encryption with SSE-KMS.</p>
    /// <p>Specifying this header with a PUT action doesn’t affect bucket-level settings for S3 Bucket Key.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn set_bucket_key_enabled(mut self, input: Option<bool>) -> Self {
        self.bucket_key_enabled = input;
        self
    }
    /// <p>Specifies whether Amazon S3 should use an S3 Bucket Key for object encryption with server-side encryption using Key Management Service (KMS) keys (SSE-KMS). Setting this header to <code>true</code> causes Amazon S3 to use an S3 Bucket Key for object encryption with SSE-KMS.</p>
    /// <p>Specifying this header with a PUT action doesn’t affect bucket-level settings for S3 Bucket Key.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn get_bucket_key_enabled(&self) -> &Option<bool> {
        &self.bucket_key_enabled
    }
    /// <p>Confirms that the requester knows that they will be charged for the request. Bucket owners need not specify this parameter in their requests. If either the source or destination S3 bucket has Requester Pays enabled, the requester will pay for corresponding charges to copy the object. For information about downloading objects from Requester Pays buckets, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/ObjectsinRequesterPaysBuckets.html">Downloading Objects in Requester Pays Buckets</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn request_payer(mut self, input: aws_sdk_s3::types::RequestPayer) -> Self {
        self.request_payer = Some(input);
        self
    }
    /// <p>Confirms that the requester knows that they will be charged for the request. Bucket owners need not specify this parameter in their requests. If either the source or destination S3 bucket has Requester Pays enabled, the requester will pay for corresponding charges to copy the object. For information about downloading objects from Requester Pays buckets, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/ObjectsinRequesterPaysBuckets.html">Downloading Objects in Requester Pays Buckets</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn set_request_payer(mut self, input: Option<aws_sdk_s3::types::RequestPayer>) -> Self {
        self.request_payer = input;
        self
    }
    /// <p>Confirms that the requester knows that they will be charged for the request. Bucket owners need not specify this parameter in their requests. If either the source or destination S3 bucket has Requester Pays enabled, the requester will pay for corresponding charges to copy the object. For information about downloading objects from Requester Pays buckets, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/ObjectsinRequesterPaysBuckets.html">Downloading Objects in Requester Pays Buckets</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn get_request_payer(&self) -> &Option<aws_sdk_s3::types::RequestPayer> {
        &self.request_payer
    }
    /// <p>The tag-set for the object. The tag-set must be encoded as URL Query parameters. (For example, "Key1=Value1")</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn tagging(mut self, input: impl Into<String>) -> Self {
        self.tagging = Some(input.into());
        self
    }
    /// <p>The tag-set for the object. The tag-set must be encoded as URL Query parameters. (For example, "Key1=Value1")</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn set_tagging(mut self, input: Option<String>) -> Self {
        self.tagging = input;
        self
    }
    /// <p>The tag-set for the object. The tag-set must be encoded as URL Query parameters. (For example, "Key1=Value1")</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn get_tagging(&self) -> &Option<String> {
        &self.tagging
    }
    /// <p>The Object Lock mode that you want to apply to this object.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn object_lock_mode(mut self, input: aws_sdk_s3::types::ObjectLockMode) -> Self {
        self.object_lock_mode = Some(input);
        self
    }
    /// <p>The Object Lock mode that you want to apply to this object.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn set_object_lock_mode(
        mut self,
        input: Option<aws_sdk_s3::types::ObjectLockMode>,
    ) -> Self {
        self.object_lock_mode = input;
        self
    }
    /// <p>The Object Lock mode that you want to apply to this object.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn get_object_lock_mode(&self) -> &Option<aws_sdk_s3::types::ObjectLockMode> {
        &self.object_lock_mode
    }
    /// <p>The date and time when you want this object's Object Lock to expire. Must be formatted as a timestamp parameter.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn object_lock_retain_until_date(mut self, input: ::aws_smithy_types::DateTime) -> Self {
        self.object_lock_retain_until_date = Some(input);
        self
    }
    /// <p>The date and time when you want this object's Object Lock to expire. Must be formatted as a timestamp parameter.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn set_object_lock_retain_until_date(
        mut self,
        input: Option<::aws_smithy_types::DateTime>,
    ) -> Self {
        self.object_lock_retain_until_date = input;
        self
    }
    /// <p>The date and time when you want this object's Object Lock to expire. Must be formatted as a timestamp parameter.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn get_object_lock_retain_until_date(&self) -> &Option<::aws_smithy_types::DateTime> {
        &self.object_lock_retain_until_date
    }
    /// <p>Specifies whether a legal hold will be applied to this object. For more information about S3 Object Lock, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/object-lock.html">Object Lock</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn object_lock_legal_hold_status(
        mut self,
        input: aws_sdk_s3::types::ObjectLockLegalHoldStatus,
    ) -> Self {
        self.object_lock_legal_hold_status = Some(input);
        self
    }
    /// <p>Specifies whether a legal hold will be applied to this object. For more information about S3 Object Lock, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/object-lock.html">Object Lock</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn set_object_lock_legal_hold_status(
        mut self,
        input: Option<aws_sdk_s3::types::ObjectLockLegalHoldStatus>,
    ) -> Self {
        self.object_lock_legal_hold_status = input;
        self
    }
    /// <p>Specifies whether a legal hold will be applied to this object. For more information about S3 Object Lock, see <a href="https://docs.aws.amazon.com/AmazonS3/latest/dev/object-lock.html">Object Lock</a> in the <i>Amazon S3 User Guide</i>.</p><note>
    /// <p>This functionality is not supported for directory buckets.</p>
    /// </note>
    pub fn get_object_lock_legal_hold_status(
        &self,
    ) -> &Option<aws_sdk_s3::types::ObjectLockLegalHoldStatus> {
        &self.object_lock_legal_hold_status
    }
    /// <p>The account ID of the expected bucket owner. If the account ID that you provide does not match the actual owner of the bucket, the request fails with the HTTP status code <code>403 Forbidden</code> (access denied).</p>
    pub fn expected_bucket_owner(mut self, input: impl Into<String>) -> Self {
        self.expected_bucket_owner = Some(input.into());
        self
    }
    /// <p>The account ID of the expected bucket owner. If the account ID that you provide does not match the actual owner of the bucket, the request fails with the HTTP status code <code>403 Forbidden</code> (access denied).</p>
    pub fn set_expected_bucket_owner(mut self, input: Option<String>) -> Self {
        self.expected_bucket_owner = input;
        self
    }
    /// <p>The account ID of the expected bucket owner. If the account ID that you provide does not match the actual owner of the bucket, the request fails with the HTTP status code <code>403 Forbidden</code> (access denied).</p>
    pub fn get_expected_bucket_owner(&self) -> &Option<String> {
        &self.expected_bucket_owner
    }

    /// Consumes the builder and constructs a [`UploadRequest`]
    // FIXME(aws-sdk-rust#1159): replace BuildError with our own type?
    pub fn build(self) -> Result<UploadRequest, ::aws_smithy_types::error::operation::BuildError> {
        Ok(UploadRequest {
            body: self.body.unwrap_or_default(),
            acl: self.acl,
            bucket: self.bucket,
            cache_control: self.cache_control,
            content_disposition: self.content_disposition,
            content_encoding: self.content_encoding,
            content_language: self.content_language,
            content_length: self.content_length,
            content_md5: self.content_md5,
            content_type: self.content_type,
            checksum_algorithm: self.checksum_algorithm,
            checksum_crc32: self.checksum_crc32,
            checksum_crc32_c: self.checksum_crc32_c,
            checksum_sha1: self.checksum_sha1,
            checksum_sha256: self.checksum_sha256,
            expires: self.expires,
            grant_full_control: self.grant_full_control,
            grant_read: self.grant_read,
            grant_read_acp: self.grant_read_acp,
            grant_write_acp: self.grant_write_acp,
            key: self.key,
            metadata: self.metadata,
            server_side_encryption: self.server_side_encryption,
            storage_class: self.storage_class,
            website_redirect_location: self.website_redirect_location,
            sse_customer_algorithm: self.sse_customer_algorithm,
            sse_customer_key: self.sse_customer_key,
            sse_customer_key_md5: self.sse_customer_key_md5,
            sse_kms_key_id: self.sse_kms_key_id,
            sse_kms_encryption_context: self.sse_kms_encryption_context,
            bucket_key_enabled: self.bucket_key_enabled,
            request_payer: self.request_payer,
            tagging: self.tagging,
            object_lock_mode: self.object_lock_mode,
            object_lock_retain_until_date: self.object_lock_retain_until_date,
            object_lock_legal_hold_status: self.object_lock_legal_hold_status,
            expected_bucket_owner: self.expected_bucket_owner,
        })
    }
}

impl Debug for UploadRequestBuilder {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        let mut formatter = f.debug_struct("UploadRequestBuilder");
        formatter.field("acl", &self.acl);
        formatter.field("body", &self.body);
        formatter.field("bucket", &self.bucket);
        formatter.field("cache_control", &self.cache_control);
        formatter.field("content_disposition", &self.content_disposition);
        formatter.field("content_encoding", &self.content_encoding);
        formatter.field("content_language", &self.content_language);
        formatter.field("content_length", &self.content_length);
        formatter.field("content_md5", &self.content_md5);
        formatter.field("content_type", &self.content_type);
        formatter.field("checksum_algorithm", &self.checksum_algorithm);
        formatter.field("checksum_crc32", &self.checksum_crc32);
        formatter.field("checksum_crc32_c", &self.checksum_crc32_c);
        formatter.field("checksum_sha1", &self.checksum_sha1);
        formatter.field("checksum_sha256", &self.checksum_sha256);
        formatter.field("expires", &self.expires);
        formatter.field("grant_full_control", &self.grant_full_control);
        formatter.field("grant_read", &self.grant_read);
        formatter.field("grant_read_acp", &self.grant_read_acp);
        formatter.field("grant_write_acp", &self.grant_write_acp);
        formatter.field("key", &self.key);
        formatter.field("metadata", &self.metadata);
        formatter.field("server_side_encryption", &self.server_side_encryption);
        formatter.field("storage_class", &self.storage_class);
        formatter.field("website_redirect_location", &self.website_redirect_location);
        formatter.field("sse_customer_algorithm", &self.sse_customer_algorithm);
        formatter.field("sse_customer_key", &"*** Sensitive Data Redacted ***");
        formatter.field("sse_customer_key_md5", &self.sse_customer_key_md5);
        formatter.field("sse_kms_key_id", &"*** Sensitive Data Redacted ***");
        formatter.field(
            "sse_kms_encryption_context",
            &"*** Sensitive Data Redacted ***",
        );
        formatter.field("bucket_key_enabled", &self.bucket_key_enabled);
        formatter.field("request_payer", &self.request_payer);
        formatter.field("tagging", &self.tagging);
        formatter.field("object_lock_mode", &self.object_lock_mode);
        formatter.field(
            "object_lock_retain_until_date",
            &self.object_lock_retain_until_date,
        );
        formatter.field(
            "object_lock_legal_hold_status",
            &self.object_lock_legal_hold_status,
        );
        formatter.field("expected_bucket_owner", &self.expected_bucket_owner);
        formatter.finish()
    }
}
