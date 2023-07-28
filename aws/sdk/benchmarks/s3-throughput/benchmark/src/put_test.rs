/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::multipart_get::get_object_multipart;
use crate::{verify, Args, BoxError, BENCH_KEY};
use async_trait::async_trait;
use aws_config::SdkConfig;
use aws_sdk_s3::Client;
use std::env::temp_dir;
use std::path::{Path, PathBuf};

pub(crate) struct PutTestResult {
    local_file: PathBuf,
    bucket: String,
    key: String,
}

#[async_trait]
pub(crate) trait PutBenchmark {
    type Setup: Send;
    async fn prepare(&self, conf: &SdkConfig) -> Self::Setup;
    async fn do_put(
        &self,
        state: Self::Setup,
        target_key: &str,
        local_file: &Path,
        args: &Args,
    ) -> Result<(), BoxError>;
    async fn do_bench(
        &self,
        state: Self::Setup,
        args: &Args,
        file: &Path,
    ) -> Result<PutTestResult, BoxError> {
        self.do_put(state, BENCH_KEY, file, args).await?;
        Ok(PutTestResult {
            local_file: file.to_path_buf(),
            bucket: args.bucket.clone(),
            key: BENCH_KEY.to_string(),
        })
    }

    async fn verify(
        &self,
        client: &Client,
        args: &Args,
        result: PutTestResult,
    ) -> Result<(), BoxError> {
        let dir = temp_dir();
        let downloaded_path = dir.join("downloaded_file");
        get_object_multipart(
            &[client.clone()][..],
            args,
            &downloaded_path,
            &result.bucket,
            &result.key,
        )
        .await?;
        verify::diff(&result.local_file, &downloaded_path).await
    }
}
