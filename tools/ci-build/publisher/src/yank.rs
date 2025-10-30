/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::cargo;
use smithy_rs_tool_common::retry::{run_with_retry, BoxError, ErrorClass};
use smithy_rs_tool_common::shell::ShellOperation;
use std::time::Duration;
use tracing::info;

#[tracing::instrument]
pub async fn yank(crate_name: &str, crate_version: &str) -> anyhow::Result<()> {
    info!("Yanking `{}-{}`...", crate_name, crate_version);
    run_with_retry(
        &format!("Yanking `{crate_name}-{crate_version}`"),
        5,
        Duration::from_secs(60),
        || async {
            cargo::Yank::new(crate_name, crate_version).spawn().await?;
            Result::<_, BoxError>::Ok(())
        },
        |_err| ErrorClass::Retry,
    )
    .await?;
    Ok(())
}
