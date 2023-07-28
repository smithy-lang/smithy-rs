/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::BoxError;
use std::path::Path;
use std::time::SystemTime;
use tokio::process::Command;

pub(crate) async fn diff(a: &Path, b: &Path) -> Result<(), BoxError> {
    let start_diff = SystemTime::now();
    let diff_ok = Command::new("diff")
        .arg(a)
        .arg(b)
        .arg("-q")
        .spawn()
        .unwrap()
        .wait()
        .await
        .unwrap();
    tracing::info!(diff_duration = ?start_diff.elapsed().unwrap());
    if !diff_ok.success() {
        Err("files differ")?
    } else {
        Ok(())
    }
}
