/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::fs::Fs;
use anyhow::{anyhow, bail, Context};
use clap::Parser;
use once_cell::sync::Lazy;
use regex::Regex;
use smithy_rs_tool_common::package::PackageStability;
use std::borrow::Cow;
use std::path::{Path, PathBuf};

static STABLE_VERSION_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?P<field>smithy\.rs\.runtime\.crate\.stable\.version=)(?P<version>\d+\.\d+\.\d+.*)",
    )
    .unwrap()
});
static UNSTABLE_VERSION_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?P<field>smithy\.rs\.runtime\.crate\.unstable\.version=)(?P<version>\d+\.\d+\.\d+.*)",
    )
    .unwrap()
});

#[derive(Parser, Debug)]
pub struct UpgradeRuntimeCratesVersionArgs {
    /// The version of stable runtime crates you want the code generator to use (e.g. `1.0.2`).
    #[clap(long)]
    stable_version: Option<String>,
    /// The version of unstable runtime crates you want the code generator to use (e.g. `0.52.0`).
    #[clap(long)]
    version: String,
    /// The path to the `gradle.properties` file. It will default to `gradle.properties` if
    /// left unspecified.
    #[clap(long, default_value = "gradle.properties")]
    gradle_properties_path: PathBuf,
}

pub async fn subcommand_upgrade_runtime_crates_version(
    args: &UpgradeRuntimeCratesVersionArgs,
) -> Result<(), anyhow::Error> {
    check_crate_ver_against_stability(&args.version, PackageStability::Unstable)?;
    let upgraded_unstable_version = semver::Version::parse(args.version.as_str())
        .with_context(|| format!("{} is not a valid semver version", &args.version))?;
    let fs = Fs::Real;
    let gradle_properties = read_gradle_properties(fs, &args.gradle_properties_path).await?;
    let updated_gradle_properties = update_gradle_properties(
        &gradle_properties,
        &upgraded_unstable_version,
        PackageStability::Unstable,
    )
    .with_context(|| {
        format!(
            "Failed to extract the expected runtime crates version from `{:?}`",
            &args.gradle_properties_path
        )
    })?;
    let updated_gradle_properties = if let Some(stable_version) = &args.stable_version {
        check_crate_ver_against_stability(stable_version, PackageStability::Stable)?;
        let upgraded_stable_version = semver::Version::parse(stable_version.as_str())
            .with_context(|| format!("{} is not a valid semver version", &stable_version))?;
        update_gradle_properties(
            &updated_gradle_properties,
            &upgraded_stable_version,
            PackageStability::Stable,
        )
        .with_context(|| {
            format!(
                "Failed to extract the expected runtime crates version from `{:?}`",
                &args.gradle_properties_path
            )
        })?
    } else {
        updated_gradle_properties
    };
    update_gradle_properties_file(
        fs,
        &args.gradle_properties_path,
        updated_gradle_properties.as_ref(),
    )
    .await?;
    Ok(())
}

fn update_gradle_properties<'a>(
    gradle_properties: &'a str,
    upgraded_version: &semver::Version,
    package_stability: PackageStability,
) -> Result<Cow<'a, str>, anyhow::Error> {
    let version_regex = match package_stability {
        PackageStability::Stable => &STABLE_VERSION_REGEX,
        PackageStability::Unstable => &UNSTABLE_VERSION_REGEX,
    };
    let current_version = version_regex
        .captures(gradle_properties)
        .ok_or_else(|| anyhow!("Failed to extract the expected runtime crates version"))?;
    let current_version = current_version.name("version").unwrap();
    let current_version = semver::Version::parse(current_version.as_str())
        .with_context(|| format!("{} is not a valid semver version", current_version.as_str()))?;
    if &current_version > upgraded_version
        // Special version tag used on the `main` branch
        && current_version != semver::Version::parse("0.0.0-smithy-rs-head").unwrap()
    {
        bail!("Moving from {current_version} to {upgraded_version} would be a *downgrade*. This command doesn't allow it!");
    }
    Ok(version_regex.replace(gradle_properties, format!("${{field}}{}", upgraded_version)))
}

async fn read_gradle_properties(fs: Fs, path: &Path) -> Result<String, anyhow::Error> {
    let bytes = fs.read_file(path).await?;
    let contents = String::from_utf8(bytes)
        .with_context(|| format!("`{:?}` contained non-UTF8 data", path))?;
    Ok(contents)
}

async fn update_gradle_properties_file(
    fs: Fs,
    path: &Path,
    contents: &str,
) -> Result<(), anyhow::Error> {
    fs.write_file(path, contents.as_bytes()).await?;
    Ok(())
}

fn check_crate_ver_against_stability(
    crate_ver: &str,
    package_stability: PackageStability,
) -> Result<(), anyhow::Error> {
    match package_stability {
        PackageStability::Stable if crate_ver.starts_with("0.") => Err(anyhow::Error::msg(
            format!("{} is an invalid stable crate version", &crate_ver),
        )),
        PackageStability::Unstable if !crate_ver.starts_with("0.") => Err(anyhow::Error::msg(
            format!("{} is an invalid unstable crate version", &crate_ver),
        )),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use crate::subcommand::upgrade_runtime_crates_version::{
        check_crate_ver_against_stability, update_gradle_properties,
    };
    use smithy_rs_tool_common::package::PackageStability;

    #[test]
    fn upgrading_works_with_actual_unstable_version() {
        let gradle_properties = "smithy.rs.runtime.crate.unstable.version=0.54.2";
        let version = semver::Version::new(0, 54, 3);
        let updated =
            update_gradle_properties(gradle_properties, &version, PackageStability::Unstable)
                .unwrap();
        assert_eq!("smithy.rs.runtime.crate.unstable.version=0.54.3", updated);
    }

    #[test]
    fn upgrading_works_with_dummy_unstable_version() {
        let gradle_properties = "smithy.rs.runtime.crate.unstable.version=0.0.0-smithy-rs-head";
        let version = semver::Version::new(0, 54, 3);
        let updated =
            update_gradle_properties(gradle_properties, &version, PackageStability::Unstable)
                .unwrap();
        assert_eq!("smithy.rs.runtime.crate.unstable.version=0.54.3", updated);
    }

    #[test]
    fn upgrading_works_with_actual_stable_version() {
        let gradle_properties = "smithy.rs.runtime.crate.stable.version=1.0.2";
        let version = semver::Version::new(1, 0, 3);
        let updated =
            update_gradle_properties(gradle_properties, &version, PackageStability::Stable)
                .unwrap();
        assert_eq!("smithy.rs.runtime.crate.stable.version=1.0.3", updated);
    }

    #[test]
    fn upgrading_works_with_dummy_stable_version() {
        let gradle_properties = "smithy.rs.runtime.crate.stable.version=0.0.0-smithy-rs-head";
        let version = semver::Version::new(1, 0, 3);
        let updated =
            update_gradle_properties(gradle_properties, &version, PackageStability::Stable)
                .unwrap();
        assert_eq!("smithy.rs.runtime.crate.stable.version=1.0.3", updated);
    }

    #[test]
    fn downgrading_stable_crate_should_be_caught_as_err() {
        let gradle_properties = "smithy.rs.runtime.crate.stable.version=1.0.2";
        let version = semver::Version::new(1, 0, 1);
        let result =
            update_gradle_properties(gradle_properties, &version, PackageStability::Stable);
        assert!(result.is_err());
        assert!(format!("{:?}", result).contains("downgrade"));
    }

    #[test]
    fn downgrading_unstable_crate_should_be_caught_as_err() {
        let gradle_properties = "smithy.rs.runtime.crate.unstable.version=0.57.1";
        let version = semver::Version::new(0, 57, 0);
        let result =
            update_gradle_properties(gradle_properties, &version, PackageStability::Unstable);
        assert!(result.is_err());
        assert!(format!("{:?}", result).contains("downgrade"));
    }

    #[test]
    fn test_check_crate_ver_against_stability() {
        assert!(check_crate_ver_against_stability("0.60.0", PackageStability::Stable).is_err());
        assert!(check_crate_ver_against_stability("1.0.0", PackageStability::Stable).is_ok());
        assert!(check_crate_ver_against_stability("2.0.0", PackageStability::Stable).is_ok());

        assert!(check_crate_ver_against_stability("0.60.0", PackageStability::Unstable).is_ok());
        assert!(check_crate_ver_against_stability("1.0.0", PackageStability::Unstable).is_err());
        assert!(check_crate_ver_against_stability("2.0.0", PackageStability::Unstable).is_err());
    }
}
