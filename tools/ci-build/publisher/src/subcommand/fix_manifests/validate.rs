/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::fs::Fs;
use crate::package::discover_and_validate_package_batches;
use anyhow::Result;
use std::path::Path;
use tracing::info;

/// Validations that run after fixing the manifests.
///
/// These should match the validations that the `publish` subcommand runs.
pub(super) async fn validate_after_fixes(location: &Path) -> Result<()> {
    info!("Post-validating manifests...");
    discover_and_validate_package_batches(Fs::Real, location).await?;
    Ok(())
}
