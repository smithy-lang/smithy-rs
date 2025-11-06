/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::cargo;
use anyhow::Result;
use smithy_rs_tool_common::{
    index::CratesIndex,
    retry::{run_with_retry, BoxError, ErrorClass},
};
use smithy_rs_tool_common::{package::PackageHandle, shell::ShellOperation};
use std::time::Duration;
use std::{path::Path, sync::Arc};
use tracing::info;

pub async fn is_published(index: Arc<CratesIndex>, crate_name: &str) -> Result<bool> {
    let crate_name = crate_name.to_string();
    let versions =
        tokio::task::spawn_blocking(move || index.published_versions(&crate_name)).await??;
    Ok(!versions.is_empty())
}

#[tracing::instrument]
pub async fn publish(handle: &PackageHandle, crate_path: &Path) -> Result<()> {
    info!("Publishing `{}`...", handle);
    run_with_retry(
        &format!("Publishing `{handle}`"),
        5,
        Duration::from_secs(60),
        || async {
            cargo::Publish::new(handle.clone(), crate_path)
                .spawn()
                .await?;
            Result::<_, BoxError>::Ok(())
        },
        |_err| ErrorClass::Retry,
    )
    .await?;
    Ok(())
}
