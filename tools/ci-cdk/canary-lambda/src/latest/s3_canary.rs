/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::canary::CanaryError;
use crate::{mk_canary, CanaryEnv};
use anyhow::{Context, Error};
use aws_config::SdkConfig;
use aws_sdk_s3 as s3;
use aws_sdk_s3::types::{
    CompletedMultipartUpload, CompletedPart, Delete, ObjectIdentifier, RequestPayer,
};
use s3::config::Region;
use s3::presigning::PresigningConfig;
use s3::primitives::ByteStream;
use std::time::Duration;
use uuid::Uuid;

const METADATA_TEST_VALUE: &str = "some   value";

mk_canary!("s3", |sdk_config: &SdkConfig, env: &CanaryEnv| {
    let sdk_config = sdk_config.clone();
    let env = env.clone();
    async move {
        let client = s3::Client::new(&sdk_config);
        s3_canary(client.clone(), env.s3_bucket_name.clone()).await?;
        s3_mrap_canary(client.clone(), env.s3_mrap_bucket_arn.clone()).await?;
        s3_express_canary(client, env.s3_express_bucket_name.clone()).await
    }
});

/// Runs canary exercising S3 APIs against a regular bucket
pub async fn s3_canary(client: s3::Client, s3_bucket_name: String) -> anyhow::Result<()> {
    let test_key = Uuid::new_v4().as_u128().to_string();
    let mut presigned_test_key = test_key.clone();
    presigned_test_key.push_str("_presigned");

    // Look for the test object and expect that it doesn't exist
    match client
        .get_object()
        .bucket(&s3_bucket_name)
        .key(&test_key)
        .send()
        .await
    {
        Ok(_) => {
            return Err(
                CanaryError(format!("Expected object {} to not exist in S3", test_key)).into(),
            );
        }
        Err(err) => {
            let err = err.into_service_error();
            // If we get anything other than "No such key", we have a problem
            if !err.is_no_such_key() {
                return Err(err).context("unexpected s3::GetObject failure");
            }
        }
    }

    // Put the test object
    client
        .put_object()
        .bucket(&s3_bucket_name)
        .key(&test_key)
        .body(ByteStream::from_static(b"test"))
        .metadata("something", METADATA_TEST_VALUE)
        .send()
        .await
        .context("s3::PutObject")?;

    // Get the test object and verify it looks correct
    let output = client
        .get_object()
        .bucket(&s3_bucket_name)
        .key(&test_key)
        .send()
        .await
        .context("s3::GetObject[2]")?;

    // Repeat the GET/PUT tests with a presigned url
    let reqwest_client = reqwest::Client::new();

    let presigned_put = client
        .put_object()
        .bucket(&s3_bucket_name)
        .key(&presigned_test_key)
        .presigned(PresigningConfig::expires_in(Duration::from_secs(120)).unwrap())
        .await
        .unwrap();
    let http_put = presigned_put.make_http_1x_request("presigned_test");
    let reqwest_put = reqwest::Request::try_from(http_put).unwrap();
    let put_resp = reqwest_client.execute(reqwest_put).await?;
    assert_eq!(put_resp.status(), 200);

    let presigned_get = client
        .get_object()
        .bucket(&s3_bucket_name)
        .key(&presigned_test_key)
        // Ensure a header is included that isn't in the query string
        .request_payer(RequestPayer::Requester)
        .presigned(PresigningConfig::expires_in(Duration::from_secs(120)).unwrap())
        .await
        .unwrap();
    let headers = presigned_get.make_http_1x_request("").headers().clone();
    let get_resp = reqwest_client
        .get(presigned_get.uri().to_string())
        .headers(headers)
        .send()
        .await
        .context("s3::presigned")?
        .text()
        .await?;
    if get_resp != "presigned_test" {
        return Err(CanaryError(format!("presigned URL returned bad data: {:?}", get_resp)).into());
    }

    let metadata_value = output
        .metadata()
        .and_then(|m| m.get("something"))
        .map(String::as_str);
    let result: anyhow::Result<()> = match metadata_value {
        Some(value) => {
            if value == METADATA_TEST_VALUE {
                let payload = output
                    .body
                    .collect()
                    .await
                    .context("download s3::GetObject[2] body")?
                    .into_bytes();
                if std::str::from_utf8(payload.as_ref()).context("s3 payload")? == "test" {
                    Ok(())
                } else {
                    Err(CanaryError("S3 object body didn't match what was put there".into()).into())
                }
            } else {
                Err(CanaryError(format!(
                    "S3 metadata was incorrect. Expected `{}` but got `{}`.",
                    METADATA_TEST_VALUE, value
                ))
                .into())
            }
        }
        None => Err(CanaryError("S3 metadata was missing".into()).into()),
    };

    // Delete the test object
    client
        .delete_object()
        .bucket(&s3_bucket_name)
        .key(&test_key)
        .send()
        .await
        .context("s3::DeleteObject")?;

    result
}

/// Runs canary exercising S3 APIs against an MRAP bucket
pub async fn s3_mrap_canary(client: s3::Client, s3_mrap_bucket_arn: String) -> anyhow::Result<()> {
    let test_key = Uuid::new_v4().as_u128().to_string();

    // We deliberately use a region that doesn't exist here so that we can
    // ensure these requests are SigV4a requests. Because the current endpoint
    // resolver always resolves the wildcard region ('*') for SigV4a requests,
    // setting a fictitious region ensures that the request would fail if it was
    // a SigV4 request. Therefore, because the request doesn't fail, we can be
    // sure it's a successful Sigv4a request.
    let config_override = s3::Config::builder().region(Region::new("parts-unknown"));
    // Put the test object
    client
        .put_object()
        .bucket(&s3_mrap_bucket_arn)
        .key(&test_key)
        .body(ByteStream::from_static(b"test"))
        .metadata("something", METADATA_TEST_VALUE)
        .customize()
        .config_override(config_override.clone())
        .send()
        .await
        .context("s3::PutObject[MRAP]")?;

    // Get the test object and verify it looks correct
    let output = client
        .get_object()
        .bucket(&s3_mrap_bucket_arn)
        .key(&test_key)
        .customize()
        .config_override(config_override.clone())
        .send()
        .await
        .context("s3::GetObject[MRAP]")?;

    let metadata_value = output
        .metadata()
        .and_then(|m| m.get("something"))
        .map(String::as_str);
    let result = match metadata_value {
        Some(value) => {
            if value == METADATA_TEST_VALUE {
                Ok(())
            } else {
                Err(CanaryError(format!(
                    "S3 metadata was incorrect. Expected `{}` but got `{}`.",
                    METADATA_TEST_VALUE, value
                ))
                .into())
            }
        }
        None => Err(CanaryError("S3 metadata was missing".into()).into()),
    };

    // Delete the test object
    client
        .delete_object()
        .bucket(&s3_mrap_bucket_arn)
        .key(&test_key)
        .customize()
        .config_override(config_override)
        .send()
        .await
        .context("s3::DeleteObject[MRAP]")?;

    result
}

/// Runs canary exercising S3 APIs against an Express One Zone bucket
pub async fn s3_express_canary(
    client: s3::Client,
    s3_express_bucket_name: String,
) -> anyhow::Result<()> {
    let test_key = Uuid::new_v4().as_u128().to_string();

    // Test a directory bucket exists
    let directory_buckets = client
        .list_directory_buckets()
        .send()
        .await
        .context("s3::ListDirectoryBuckets[Express]")?;
    assert!(directory_buckets
        .buckets
        .map(|buckets| buckets
            .iter()
            .any(|b| b.name() == Some(&s3_express_bucket_name)))
        .expect("true"));

    // Check test object does not exist in the directory bucket
    let list_objects_v2_output = client
        .list_objects_v2()
        .bucket(&s3_express_bucket_name)
        .send()
        .await
        .context("s3::ListObjectsV2[EXPRESS]")?;
    match list_objects_v2_output.contents {
        Some(contents) => {
            // should the directory bucket contains some leftover object,
            // it better not be the test object
            assert!(!contents.iter().any(|c| c.key() == Some(&test_key)));
        }
        _ => { /* No objects in the directory bucket, good to go */ }
    }

    // Put the test object
    client
        .put_object()
        .bucket(&s3_express_bucket_name)
        .key(&test_key)
        .body(ByteStream::from_static(b"test"))
        .metadata("something", METADATA_TEST_VALUE)
        .send()
        .await
        .context("s3::PutObject[EXPRESS]")?;

    // Get the test object and verify it looks correct
    let output = client
        .get_object()
        .bucket(&s3_express_bucket_name)
        .key(&test_key)
        .send()
        .await
        .context("s3::GetObject[EXPRESS]")?;

    // repeat the test with a presigned url
    let uri = client
        .get_object()
        .bucket(&s3_express_bucket_name)
        .key(&test_key)
        .presigned(PresigningConfig::expires_in(Duration::from_secs(120)).unwrap())
        .await
        .unwrap();
    let response = reqwest::get(uri.uri().to_string())
        .await
        .context("s3::presigned")?
        .text()
        .await?;
    if response != "test" {
        return Err(CanaryError(format!("presigned URL returned bad data: {:?}", response)).into());
    }

    let metadata_value = output
        .metadata()
        .and_then(|m| m.get("something"))
        .map(String::as_str);
    let result: anyhow::Result<()> = match metadata_value {
        Some(value) => {
            if value == METADATA_TEST_VALUE {
                let payload = output
                    .body
                    .collect()
                    .await
                    .context("download s3::GetObject[EXPRESS] body")?
                    .into_bytes();
                if std::str::from_utf8(payload.as_ref()).context("s3 payload")? == "test" {
                    Ok(())
                } else {
                    Err(CanaryError("S3 object body didn't match what was put there".into()).into())
                }
            } else {
                Err(CanaryError(format!(
                    "S3 metadata was incorrect. Expected `{}` but got `{}`.",
                    METADATA_TEST_VALUE, value
                ))
                .into())
            }
        }
        None => Err(CanaryError("S3 metadata was missing".into()).into()),
    };

    result?;

    // Delete the test object
    client
        .delete_object()
        .bucket(&s3_express_bucket_name)
        .key(&test_key)
        .send()
        .await
        .context("s3::DeleteObject[EXPRESS]")?;

    // Another key for MultipartUpload (verifying default checksum is None)
    let test_key = Uuid::new_v4().as_u128().to_string();

    // Create multipart upload
    let create_mpu_output = client
        .create_multipart_upload()
        .bucket(&s3_express_bucket_name)
        .key(&test_key)
        .send()
        .await
        .unwrap();
    let upload_id = create_mpu_output
        .upload_id()
        .context("s3::CreateMultipartUpload[EXPRESS]")?;

    // Upload part
    let upload_part_output = client
        .upload_part()
        .bucket(&s3_express_bucket_name)
        .key(&test_key)
        .part_number(1)
        .body(ByteStream::from_static(b"test"))
        .upload_id(upload_id)
        .send()
        .await
        .context("s3::UploadPart[EXPRESS]")?;

    // Complete multipart upload
    client
        .complete_multipart_upload()
        .bucket(&s3_express_bucket_name)
        .key(&test_key)
        .upload_id(upload_id)
        .multipart_upload(
            CompletedMultipartUpload::builder()
                .set_parts(Some(vec![CompletedPart::builder()
                    .e_tag(upload_part_output.e_tag.unwrap_or_default())
                    .part_number(1)
                    .build()]))
                .build(),
        )
        .send()
        .await
        .context("s3::CompleteMultipartUpload[EXPRESS]")?;

    // Delete test objects using the DeleteObjects operation whose default checksum should be CRC32
    client
        .delete_objects()
        .bucket(&s3_express_bucket_name)
        .delete(
            Delete::builder()
                .objects(
                    ObjectIdentifier::builder()
                        .key(&test_key)
                        .build()
                        .context("failed to build `ObjectIdentifier`")?,
                )
                .build()
                .context("failed to build `Delete`")?,
        )
        .send()
        .await
        .context("s3::DeleteObjects[EXPRESS]")?;

    Ok::<(), Error>(())
}

// This test runs against an actual AWS account. Comment out the `ignore` to run it.
// Be sure the following environment variables are set:
//
// - `TEST_S3_BUCKET`: The S3 bucket to use
// - `TEST_S3_MRAP_BUCKET_ARN`: The MRAP bucket ARN to use
// - `TEST_S3_EXPRESS_BUCKET`: The S3 express bucket to use
//
// Also, make sure the correct region (likely `us-west-2`) by the credentials or explictly.
#[ignore]
#[cfg(test)]
#[tokio::test]
async fn test_s3_canary() {
    let config = aws_config::load_from_env().await;
    let client = s3::Client::new(&config);

    let mut futures = Vec::new();

    futures.push(tokio::spawn(s3_canary(
        client.clone(),
        std::env::var("TEST_S3_BUCKET").expect("TEST_S3_BUCKET must be set"),
    )));
    futures.push(tokio::spawn(s3_mrap_canary(
        client.clone(),
        std::env::var("TEST_S3_MRAP_BUCKET_ARN").expect("TEST_S3_MRAP_BUCKET_ARN must be set"),
    )));
    futures.push(tokio::spawn(s3_express_canary(
        client,
        std::env::var("TEST_S3_EXPRESS_BUCKET").expect("TEST_S3_EXPRESS_BUCKET must be set"),
    )));

    for fut in futures {
        fut.await.expect("joined").expect("success");
    }
}
