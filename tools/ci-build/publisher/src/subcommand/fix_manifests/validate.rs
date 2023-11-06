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

    let expected_stable_runtime_version = versions
        .get("aws-smithy-types")
        .ok_or_else(|| anyhow!("`aws-smithy-types` crate missing"))?;

    let expected_unstable_runtime_version = versions
        .get("aws-smithy-http")
        .ok_or_else(|| anyhow!("`aws-smithy-http` crate missing"))?;

    for (name, version) in versions.published_crates() {
        let category = PackageCategory::from_package_name(name);
        if category == PackageCategory::SmithyRuntime || category == PackageCategory::AwsRuntime {
            let expected_runtime_version = if let Some(true) = versions.stable(name) {
                expected_stable_runtime_version
            } else {
                expected_unstable_runtime_version
            };
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
    use smithy_rs_tool_common::package::PackageStability;
    use std::collections::BTreeMap;
    use std::str::FromStr;

    fn versions(versions: &[(&'static str, &'static str, PackageStability)]) -> Versions {
        let mut map = BTreeMap::new();
        for (name, version, stability) in versions {
            map.insert(
                (*name).into(),
                VersionWithMetadata {
                    version: Version::from_str(version).unwrap(),
                    publish: true,
                    stability: *stability,
                },
            );
        }
        Versions(map)
    }

    #[track_caller]
    fn expect_success(version_tuples: &[(&'static str, &'static str, PackageStability)]) {
        validate_before_fixes(&versions(version_tuples), false).expect("success");
    }

    #[track_caller]
    fn expect_failure(
        message: &str,
        version_tuples: &[(&'static str, &'static str, PackageStability)],
    ) {
        if let Err(err) = validate_before_fixes(&versions(version_tuples), false) {
            assert_eq!(message, format!("{}", err));
        } else {
            panic!("Expected validation failure");
        }
    }

    #[test]
    fn pre_validate() {
        expect_success(&[
            ("aws-config", "1.5.2", PackageStability::Stable),
            ("aws-smithy-http", "0.35.1", PackageStability::Unstable),
            ("aws-sdk-s3", "1.5.2", PackageStability::Stable),
            ("aws-smithy-types", "1.5.2", PackageStability::Stable),
            ("aws-types", "1.5.2", PackageStability::Stable),
        ]);

        expect_success(&[
            ("aws-smithy-types", "1.5.2", PackageStability::Stable),
            ("aws-smithy-http", "0.35.1", PackageStability::Unstable),
        ]);

        expect_failure(
            "Crate named `aws-smithy-runtime-api` should be at version `1.5.3` but is at `1.5.2`",
            &[
                ("aws-smithy-runtime-api", "1.5.2", PackageStability::Stable),
                ("aws-smithy-types", "1.5.3", PackageStability::Stable),
                ("aws-smithy-http", "0.35.0", PackageStability::Unstable),
            ],
        );

        expect_success(&[
            ("aws-config", "1.5.2", PackageStability::Stable),
            ("aws-smithy-http", "0.35.0", PackageStability::Unstable),
            ("aws-sdk-s3", "1.5.2", PackageStability::Stable),
            ("aws-smithy-types", "1.5.2", PackageStability::Stable),
            ("aws-types", "1.5.2", PackageStability::Stable),
        ]);

        expect_failure(
            "Crate named `aws-types` should be at version `1.5.3` but is at `1.5.2`",
            &[
                ("aws-config", "1.5.3", PackageStability::Stable),
                ("aws-sdk-s3", "1.5.3", PackageStability::Stable),
                ("aws-smithy-http", "0.35.0", PackageStability::Unstable),
                ("aws-smithy-types", "1.5.3", PackageStability::Stable),
                ("aws-types", "1.5.2", PackageStability::Stable),
            ],
        );

        expect_success(&[
            ("aws-config", "1.5.3", PackageStability::Stable),
            ("aws-sdk-s3", "1.5.3", PackageStability::Stable),
            ("aws-smithy-types", "1.5.3", PackageStability::Stable),
            ("aws-smithy-http", "0.35.0", PackageStability::Unstable),
        ]);
    }
}
