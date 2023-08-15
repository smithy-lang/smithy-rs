/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::canary::CanaryError;
use crate::{mk_canary, CanaryEnv};
use anyhow::Context;
use aws_config::SdkConfig;
use aws_sdk_s3 as s3;
use s3::presigning::PresigningConfig;
use s3::primitives::ByteStream;
use std::time::Duration;
use uuid::Uuid;

const METADATA_TEST_VALUE: &str = "some   value";

mk_canary!("s3", |sdk_config: &SdkConfig, env: &CanaryEnv| s3_canary(
    s3::Client::new(sdk_config),
    env.s3_bucket_name.clone()
));

pub async fn s3_canary(client: s3::Client, s3_bucket_name: String) -> anyhow::Result<()> {
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

    let mut result = Ok(());
    match output.metadata() {
        Some(map) => {
            // Option::as_deref doesn't work here since the deref of &String is String
            let value = map.get("something").map(|s| s.as_str()).unwrap_or("");
            if value != METADATA_TEST_VALUE {
                result = Err(CanaryError(format!(
                    "S3 metadata was incorrect. Expected `{}` but got `{}`.",
                    METADATA_TEST_VALUE, value
                ))
                .into());
            }
        }
        None => {
            result = Err(CanaryError("S3 metadata was missing".into()).into());
        }
    }

    let payload = output
        .body
        .collect()
        .await
        .context("download s3::GetObject[2] body")?
        .into_bytes();
    if std::str::from_utf8(payload.as_ref()).context("s3 payload")? != "test" {
        result = Err(CanaryError("S3 object body didn't match what was put there".into()).into());
    }

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

// This test runs against an actual AWS account. Comment out the `ignore` to run it.
// Be sure to set the `TEST_S3_BUCKET` environment variable to the S3 bucket to use,
// and also make sure the credential profile sets the region (or set `AWS_DEFAULT_PROFILE`).
#[ignore]
#[cfg(test)]
#[tokio::test]
async fn test_s3_canary() {
    let config = aws_config::load_from_env().await;
    let client = s3::Client::new(&config);
    s3_canary(
        client,
        std::env::var("TEST_S3_BUCKET").expect("TEST_S3_BUCKET must be set"),
    )
    .await
    .expect("success");
}
