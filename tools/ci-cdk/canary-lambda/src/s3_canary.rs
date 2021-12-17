/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::canary::CanaryError;
use anyhow::Context;
use aws_sdk_s3 as s3;
use uuid::Uuid;

pub async fn s3_canary(client: s3::Client, s3_bucket_name: String) -> anyhow::Result<()> {
    use s3::{error::GetObjectError, error::GetObjectErrorKind, SdkError};
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
        Err(SdkError::ServiceError {
            err:
                GetObjectError {
                    kind: GetObjectErrorKind::NoSuchKey { .. },
                    ..
                },
            ..
        }) => {
            // good
        }
        Err(err) => {
            Err(err).context("unexpected s3::GetObject failure")?;
        }
    }

    // Put the test object
    client
        .put_object()
        .bucket(&s3_bucket_name)
        .key(&test_key)
        .body(s3::ByteStream::from_static(b"test"))
        .metadata("something", "テスト テスト!")
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

    let mut result = Ok(());
    match output.metadata() {
        Some(map) => {
            // Option::as_deref doesn't work here since the deref of &String is String
            let value = map.get("something").map(|s| s.as_str()).unwrap_or("");
            if value != "テスト テスト!" {
                result = Err(CanaryError(format!(
                    "S3 metadata was incorrect. Expected `テスト テスト!` but got `{}`.",
                    value
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
