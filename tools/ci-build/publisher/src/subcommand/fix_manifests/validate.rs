/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::fs::Fs;
use crate::package::discover_and_validate_package_batches;
use crate::subcommand::fix_manifests::Versions;
use anyhow::{anyhow, bail, Result};
use semver::Version;
use smithy_rs_tool_common::package::PackageCategory;
use std::path::Path;
use tracing::info;

/// Validations that run before the manifests are fixed.
///
/// For now, this validates:
/// - `aws-smithy-` prefixed versions match `aws-` (NOT `aws-sdk-`) prefixed versions
pub(super) fn validate_before_fixes(
    versions: &Versions,
    disable_version_number_validation: bool,
) -> Result<()> {
    // Later when we only generate independently versioned SDK crates, this flag can become permanent.
    if disable_version_number_validation {
        return Ok(());
    }

    info!("Pre-validation manifests...");
    let expected_runtime_version = versions
        .get("aws-smithy-types")
        .ok_or_else(|| anyhow!("`aws-smithy-types` crate missing"))?;

    for (name, version) in versions.published_crates() {
        let category = PackageCategory::from_package_name(name);
        if category == PackageCategory::SmithyRuntime || category == PackageCategory::AwsRuntime {
            confirm_version(name, expected_runtime_version, version)?;
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
pub(super) async fn validate_after_fixes(location: &Path) -> Result<()> {
    info!("Post-validating manifests...");
    discover_and_validate_package_batches(Fs::Real, location).await?;
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::subcommand::fix_manifests::VersionWithMetadata;
    use std::collections::BTreeMap;
    use std::str::FromStr;

    fn versions(versions: &[(&'static str, &'static str)]) -> Versions {
        let mut map = BTreeMap::new();
        for (name, version) in versions {
            map.insert(
                (*name).into(),
                VersionWithMetadata {
                    version: Version::from_str(version).unwrap(),
                    publish: true,
                },
            );
        }
        Versions(map)
    }

    #[track_caller]
    fn expect_success(version_tuples: &[(&'static str, &'static str)]) {
        validate_before_fixes(&versions(version_tuples), false).expect("success");
    }

    #[track_caller]
    fn expect_failure(message: &str, version_tuples: &[(&'static str, &'static str)]) {
        if let Err(err) = validate_before_fixes(&versions(version_tuples), false) {
            assert_eq!(message, format!("{}", err));
        } else {
            panic!("Expected validation failure");
        }
    }

    #[test]
    fn pre_validate() {
        expect_success(&[
            ("aws-config", "0.35.1"),
            ("aws-sdk-s3", "0.5.1"),
            ("aws-smithy-types", "0.35.1"),
            ("aws-types", "0.35.1"),
        ]);

        expect_success(&[
            ("aws-smithy-types", "0.35.1"),
            ("aws-smithy-http", "0.35.1"),
            ("aws-smithy-client", "0.35.1"),
        ]);

        expect_failure(
            "Crate named `aws-smithy-http` should be at version `0.35.1` but is at `0.35.0`",
            &[
                ("aws-smithy-types", "0.35.1"),
                ("aws-smithy-http", "0.35.0"),
                ("aws-smithy-client", "0.35.1"),
            ],
        );

        expect_success(&[
            ("aws-config", "0.35.1"),
            ("aws-sdk-s3", "0.5.0"),
            ("aws-smithy-types", "0.35.1"),
            ("aws-types", "0.35.1"),
        ]);

        expect_failure(
            "Crate named `aws-types` should be at version `0.35.1` but is at `0.35.0`",
            &[
                ("aws-config", "0.35.1"),
                ("aws-sdk-s3", "0.5.1"),
                ("aws-smithy-types", "0.35.1"),
                ("aws-types", "0.35.0"),
            ],
        );

        expect_failure(
            "Crate named `aws-smithy-http` should be at version `0.35.1` but is at `0.35.0`",
            &[
                ("aws-config", "0.35.1"),
                ("aws-sdk-s3", "0.5.1"),
                ("aws-smithy-types", "0.35.1"),
                ("aws-smithy-http", "0.35.0"),
            ],
        );
    }
}
