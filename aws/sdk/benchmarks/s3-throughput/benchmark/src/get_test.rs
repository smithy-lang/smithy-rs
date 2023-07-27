/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::{verify, Args, BoxError};
use async_trait::async_trait;
use aws_config::SdkConfig;
use aws_sdk_s3::Client;
use std::path::{Path, PathBuf};

pub(crate) struct GetTestResult {
    pub(crate) expected: PathBuf,
    pub(crate) actual: PathBuf,
}

#[async_trait]
pub(crate) trait GetBenchmark {
    type Setup: Send;
    async fn prepare(&self, conf: &SdkConfig) -> Self::Setup;
    async fn do_get(
        &self,
        state: Self::Setup,
        target_path: &Path,
        args: &Args,
    ) -> Result<PathBuf, BoxError>;
    async fn do_bench(
        &self,
        state: Self::Setup,
        args: &Args,
        expected_path: &Path,
    ) -> Result<GetTestResult, BoxError> {
        let target_path = expected_path.with_extension("downloaded");
        let downloaded_path = self.do_get(state, &target_path, args).await?;
        Ok(GetTestResult {
            expected: expected_path.to_path_buf(),
            actual: downloaded_path,
        })
    }

    async fn verify(
        &self,
        _client: &Client,
        _args: &Args,
        result: GetTestResult,
    ) -> Result<(), BoxError> {
        verify::diff(&result.actual, &result.expected).await
    }
}
