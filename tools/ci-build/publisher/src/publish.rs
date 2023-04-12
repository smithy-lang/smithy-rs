/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::cargo;
use crate::package::PackageHandle;
use crate::retry::{run_with_retry, BoxError, ErrorClass};
use crates_io_api::{AsyncClient, Error};
use lazy_static::lazy_static;
use smithy_rs_tool_common::shell::ShellOperation;
use std::path::Path;
use std::time::Duration;
use tracing::info;

lazy_static! {
    pub static ref CRATES_IO_CLIENT: AsyncClient = AsyncClient::new(
        "AWS_RUST_SDK_PUBLISHER (aws-sdk-rust@amazon.com)",
        Duration::from_secs(1)
    )
    .expect("valid client");
}

/// Return `true` if there is at least one version published on crates.io associated with
/// the specified crate name.
#[tracing::instrument]
pub async fn has_been_published_on_crates_io(crate_name: &str) -> anyhow::Result<bool> {
    match CRATES_IO_CLIENT.get_crate(crate_name).await {
        Ok(_) => Ok(true),
        Err(Error::NotFound(_)) => Ok(false),
        Err(e) => Err(e.into()),
    }
}

#[tracing::instrument]
pub async fn publish(handle: &PackageHandle, crate_path: &Path) -> anyhow::Result<()> {
    info!("Publishing `{}`...", handle);
    run_with_retry(
        &format!("Publishing `{}`", handle),
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
