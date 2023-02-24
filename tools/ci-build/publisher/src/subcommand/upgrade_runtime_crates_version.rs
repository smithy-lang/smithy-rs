/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::fs::Fs;
use anyhow::{anyhow, bail, Context};
use clap::Parser;
use regex::Regex;
use std::borrow::Cow;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
pub struct UpgradeRuntimeCratesVersionArgs {
    /// The version of runtime crates you want the code generator to use (e.g. `0.52.0`).
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
    let upgraded_version = semver::Version::parse(args.version.as_str())
        .with_context(|| format!("{} is not a valid semver version", &args.version))?;
    let fs = Fs::Real;
    let gradle_properties = read_gradle_properties(fs, &args.gradle_properties_path).await?;
    let updated_gradle_properties = update_gradle_properties(&gradle_properties, &upgraded_version)
        .with_context(|| {
            format!(
                "Failed to extract the expected runtime crates version from `{:?}`",
                &args.gradle_properties_path
            )
        })?;
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
    upgraded_version: &'a semver::Version,
) -> Result<Cow<'a, str>, anyhow::Error> {
    let version_regex =
        Regex::new(r"(?P<field>smithy\.rs\.runtime\.crate\.version=)(?P<version>\d+\.\d+\.\d+.*)")
            .unwrap();
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

#[cfg(test)]
mod tests {
    use crate::subcommand::upgrade_runtime_crates_version::update_gradle_properties;

    #[test]
    fn upgrading_works_with_actual_version() {
        let gradle_properties = "smithy.rs.runtime.crate.version=0.54.2";
        let version = semver::Version::new(0, 54, 3);
        let updated = update_gradle_properties(gradle_properties, &version).unwrap();
        assert_eq!("smithy.rs.runtime.crate.version=0.54.3", updated);
    }

    #[test]
    fn upgrading_works_with_dummy_version() {
        let gradle_properties = "smithy.rs.runtime.crate.version=0.0.0-smithy-rs-head";
        let version = semver::Version::new(0, 54, 3);
        let updated = update_gradle_properties(gradle_properties, &version).unwrap();
        assert_eq!("smithy.rs.runtime.crate.version=0.54.3", updated);
    }
}
