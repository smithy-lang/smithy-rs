/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::fs::Fs;
use crate::package::discover_and_validate_package_batches;
use anyhow::{anyhow, bail, Result};
use semver::Version;
use std::collections::BTreeMap;
use tracing::info;

/// Validations that run before the manifests are fixed.
///
/// For now, this validates:
/// - `aws-config` version number matches all `aws-sdk-` prefixed versions
/// - `aws-smithy-` prefixed versions match `aws-` (NOT `aws-sdk-`) prefixed versions
pub(super) fn pre_validate_manifests(versions: &BTreeMap<String, Version>) -> Result<()> {
    info!("Pre-validation manifests...");
    let expected_sdk_version = versions
        .get("aws-config")
        .ok_or_else(|| anyhow!("`aws-config` crate missing"))?;
    let expected_smithy_version = versions
        .get("aws-smithy-types")
        .ok_or_else(|| anyhow!("`aws-smithy-types` crate missing"))?;

    for (name, version) in versions {
        if name.starts_with("aws-smithy-") {
            confirm_version(name, expected_smithy_version, version)?;
        } else if name.starts_with("aws-") {
            confirm_version(name, expected_sdk_version, version)?;
        }
    }
    Ok(())
}

fn confirm_version(name: &str, expected: &Version, actual: &Version) -> Result<()> {
    if expected != actual {
        bail!(
            "Crate named `{}` should be at version `{}` but is at `{}`",
            name,
            expected,
            actual
        );
    }
    Ok(())
}

/// Validations that run after fixing the manifests.
///
/// These should match the validations that the `publish` subcommand runs.
pub(super) async fn post_validate_manifests(location: &str) -> Result<()> {
    info!("Post-validating manifests...");
    discover_and_validate_package_batches(Fs::Real, location).await?;
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use std::str::FromStr;

    fn versions(versions: &[(&'static str, &'static str)]) -> BTreeMap<String, Version> {
        let mut map = BTreeMap::new();
        for (name, version) in versions {
            map.insert((*name).into(), Version::from_str(version).unwrap());
        }
        map
    }

    #[track_caller]
    fn expect_success(version_tuples: &[(&'static str, &'static str)]) {
        pre_validate_manifests(&versions(version_tuples)).expect("success");
    }

    #[track_caller]
    fn expect_failure(message: &str, version_tuples: &[(&'static str, &'static str)]) {
        if let Err(err) = pre_validate_manifests(&versions(version_tuples)) {
            assert_eq!(message, format!("{}", err));
        } else {
            panic!("Expected validation failure");
        }
    }

    #[test]
    fn pre_validate() {
        expect_success(&[
            ("aws-config", "0.5.1"),
            ("aws-sdk-s3", "0.5.1"),
            ("aws-smithy-types", "0.35.1"),
            ("aws-types", "0.5.1"),
        ]);

        expect_failure(
            "Crate named `aws-sdk-s3` should be at version `0.5.1` but is at `0.5.0`",
            &[
                ("aws-config", "0.5.1"),
                ("aws-sdk-s3", "0.5.0"),
                ("aws-smithy-types", "0.35.1"),
                ("aws-types", "0.5.1"),
            ],
        );

        expect_failure(
            "Crate named `aws-types` should be at version `0.5.1` but is at `0.5.0`",
            &[
                ("aws-config", "0.5.1"),
                ("aws-sdk-s3", "0.5.1"),
                ("aws-smithy-types", "0.35.1"),
                ("aws-types", "0.5.0"),
            ],
        );

        expect_failure(
            "Crate named `aws-smithy-http` should be at version `0.35.1` but is at `0.35.0`",
            &[
                ("aws-config", "0.5.1"),
                ("aws-sdk-s3", "0.5.1"),
                ("aws-smithy-types", "0.35.1"),
                ("aws-smithy-http", "0.35.0"),
            ],
        );
    }
}
