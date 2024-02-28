/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::canary::CanaryError;
use crate::{mk_canary, CanaryEnv};
use anyhow::Context;
use aws_config::SdkConfig;
use aws_sdk_s3 as s3;
use s3::config::Region;
use s3::presigning::PresigningConfig;
use s3::primitives::ByteStream;
use std::time::Duration;
use uuid::Uuid;

const METADATA_TEST_VALUE: &str = "some   value";

mk_canary!("s3", |sdk_config: &SdkConfig, env: &CanaryEnv| s3_canary(
    s3::Client::new(sdk_config),
    env.s3_bucket_name.clone(),
    env.s3_mrap_bucket_arn.clone()
));

pub async fn s3_canary(
    client: s3::Client,
    s3_bucket_name: String,
    s3_mrap_bucket_arn: String,
) -> anyhow::Result<()> {
    let test_key = Uuid::new_v4().as_u128().to_string();

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

    // repeat the test with a presigned url
    let uri = client
        .get_object()
        .bucket(&s3_bucket_name)
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

    // Return early if the result is an error
    result?;

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
        .context("s3::DeleteObject")?;

    result
}

// This test runs against an actual AWS account. Comment out the `ignore` to run it.
// Be sure the following environment variables are set:
//
// - `TEST_S3_BUCKET`: The S3 bucket to use
// - `TEST_S3_MRAP_BUCKET_ARN`: The MRAP bucket ARN to use
//
// Also, make sure the correct region (likely `us-west-2`) by the credentials or explictly.
#[ignore]
#[cfg(test)]
#[tokio::test]
async fn test_s3_canary() {
    let config = aws_config::load_from_env().await;
    let client = s3::Client::new(&config);
    s3_canary(
        client,
        std::env::var("TEST_S3_BUCKET").expect("TEST_S3_BUCKET must be set"),
        std::env::var("TEST_S3_MRAP_BUCKET_ARN").expect("TEST_S3_MRAP_BUCKET_ARN must be set"),
    )
    .await
    .expect("success");
}
