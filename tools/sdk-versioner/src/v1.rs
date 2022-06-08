/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! This is all deprecated code kept around so that older commits can be code generated with the latest tools
//! to unblock a release. It can be deleted after the first successful release after this file was introduced.

use anyhow::bail;
use clap::Parser;
use smithy_rs_tool_common::package::{PackageCategory, SDK_PREFIX};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use toml::value::{Table, Value};

#[derive(Parser, Debug)]
#[clap(
    name = "sdk-versioner",
    about = "CLI tool to recursively update SDK/Smithy crate references in Cargo.toml files",
    version
)]
pub struct Args {
    /// Path(s) to recursively update Cargo.toml files in
    #[clap()]
    crate_paths: Vec<PathBuf>,

    /// SDK version to point to
    #[clap(long)]
    sdk_version: Option<String>,
    /// Smithy version to point to
    #[clap(long)]
    smithy_version: Option<String>,

    /// Path to generated SDK to point to
    #[clap(long)]
    sdk_path: Option<PathBuf>,
}

impl Args {
    fn validate(self) -> anyhow::Result<Self> {
        if self.crate_paths.is_empty() {
            bail!("Must provide at least one crate path to recursively update");
        }
        if self.sdk_version.is_none() && self.sdk_path.is_none() {
            bail!("Must provide either an SDK version or an SDK path to update to");
        }
        if self.sdk_version.is_some() != self.smithy_version.is_some() {
            bail!("Must provide a Smithy version when providing an SDK version to update to");
        }
        Ok(self)
    }
}

pub fn main() -> anyhow::Result<()> {
    let opt = Args::parse().validate()?;

    let start_time = Instant::now();
    let mut manifest_paths = Vec::new();
    for crate_path in &opt.crate_paths {
        discover_manifests(&mut manifest_paths, crate_path)?;
    }

    for manifest_path in manifest_paths {
        update_manifest(&manifest_path, &opt)?;
    }

    println!("Finished in {:?}", start_time.elapsed());
    Ok(())
}

fn update_manifest(manifest_path: &Path, opt: &Args) -> anyhow::Result<()> {
    println!("Updating {:?}...", manifest_path);

    let mut metadata: Value = toml::from_slice(&fs::read(manifest_path)?)?;
    let mut changed = false;
    for set in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(dependencies) = metadata.get_mut(set) {
            if !dependencies.is_table() {
                bail!(
                    "Unexpected non-table value named `{}` in {:?}",
                    set,
                    manifest_path
                );
            }
            changed = update_dependencies(dependencies.as_table_mut().unwrap(), opt)? || changed;
        }
    }

    if changed {
        fs::write(manifest_path, &toml::to_vec(&metadata)?)?;
    }

    Ok(())
}

fn update_dependencies(dependencies: &mut Table, opt: &Args) -> anyhow::Result<bool> {
    let mut changed = false;
    for (key, value) in dependencies.iter_mut() {
        let category = PackageCategory::from_package_name(key);
        if !matches!(category, PackageCategory::Unknown) {
            if !value.is_table() {
                *value = Value::Table(Table::new());
            }
            update_dependency_value(key, value.as_table_mut().unwrap(), opt);
            changed = true;
        }
    }
    Ok(changed)
}

fn crate_path_name(name: &str) -> &str {
    if matches!(
        PackageCategory::from_package_name(name),
        PackageCategory::AwsSdk
    ) {
        &name[SDK_PREFIX.len()..]
    } else {
        name
    }
}

fn update_dependency_value(crate_name: &str, value: &mut Table, opt: &Args) {
    let is_sdk_crate = matches!(
        PackageCategory::from_package_name(crate_name),
        PackageCategory::AwsSdk | PackageCategory::AwsRuntime,
    );

    // Remove keys that will be replaced
    value.remove("git");
    value.remove("branch");
    value.remove("version");
    value.remove("path");

    // Set the `path` if one was given
    if let Some(path) = &opt.sdk_path {
        let crate_path = path.join(crate_path_name(crate_name));
        value.insert(
            "path".to_string(),
            Value::String(
                crate_path
                    .as_os_str()
                    .to_str()
                    .expect("valid utf-8 path")
                    .to_string(),
            ),
        );
    }

    // Set the `version` if one was given
    if opt.sdk_version.is_some() {
        value.insert(
            "version".to_string(),
            Value::String(
                if is_sdk_crate {
                    &opt.sdk_version
                } else {
                    &opt.smithy_version
                }
                .clone()
                .unwrap(),
            ),
        );
    }
}

/// Recursively discovers Cargo.toml files in the given `path` and adds them to `manifests`.
fn discover_manifests(manifests: &mut Vec<PathBuf>, path: impl AsRef<Path>) -> anyhow::Result<()> {
    let path = path.as_ref();

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if entry.path().is_dir() {
            discover_manifests(manifests, entry.path())?;
        } else if entry.path().is_file()
            && entry.path().file_name() == Some(OsStr::new("Cargo.toml"))
        {
            manifests.push(entry.path());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{update_manifest, Args};
    use pretty_assertions::assert_eq;
    use toml::Value;

    const TEST_MANIFEST: &[u8] = br#"
        [package]
        name = "test"
        version = "0.1.0"
        [dependencies]
        aws-config = "0.4.1"
        aws-sdk-s3 = "0.4.1"
        aws-smithy-types = "0.34.1"
        aws-smithy-http = { version = "0.34.1", features = ["test-util"] }
        something-else = "0.1"
    "#;

    #[track_caller]
    fn test_with_opt(opt: Args, expected: &[u8]) {
        let manifest_file = tempfile::NamedTempFile::new().unwrap();
        let manifest_path = manifest_file.into_temp_path();
        std::fs::write(&manifest_path, TEST_MANIFEST).unwrap();

        update_manifest(&manifest_path, &opt).expect("success");

        let actual = toml::from_slice(&std::fs::read(&manifest_path).expect("read tmp file"))
            .expect("valid toml");
        let expected: Value = toml::from_slice(expected).unwrap();
        assert_eq!(expected, actual);
    }

    #[test]
    fn update_dependencies_with_versions() {
        test_with_opt(
            Args {
                crate_paths: Vec::new(),
                sdk_path: None,
                sdk_version: Some("0.5.0".to_string()),
                smithy_version: Some("0.35.0".to_string()),
            },
            br#"
            [package]
            name = "test"
            version = "0.1.0"
            [dependencies]
            aws-config = { version = "0.5.0" }
            aws-sdk-s3 = { version = "0.5.0" }
            aws-smithy-types = { version = "0.35.0" }
            aws-smithy-http = { version = "0.35.0", features = ["test-util"] }
            something-else = "0.1"
            "#,
        );
    }

    #[test]
    fn update_dependencies_with_paths() {
        test_with_opt(
            Args {
                crate_paths: Vec::new(),
                sdk_path: Some("/foo/asdf/".into()),
                sdk_version: None,
                smithy_version: None,
            },
            br#"
            [package]
            name = "test"
            version = "0.1.0"
            [dependencies]
            aws-config = { path = "/foo/asdf/aws-config" }
            aws-sdk-s3 = { path = "/foo/asdf/s3" }
            aws-smithy-types = { path = "/foo/asdf/aws-smithy-types" }
            aws-smithy-http = { path = "/foo/asdf/aws-smithy-http", features = ["test-util"] }
            something-else = "0.1"
            "#,
        );
    }

    #[test]
    fn update_dependencies_with_versions_and_paths() {
        test_with_opt(
            Args {
                crate_paths: Vec::new(),
                sdk_path: Some("/foo/asdf/".into()),
                sdk_version: Some("0.5.0".to_string()),
                smithy_version: Some("0.35.0".to_string()),
            },
        br#"
            [package]
            name = "test"
            version = "0.1.0"
            [dependencies]
            aws-config = { version = "0.5.0", path = "/foo/asdf/aws-config" }
            aws-sdk-s3 = { version = "0.5.0", path = "/foo/asdf/s3" }
            aws-smithy-types = { version = "0.35.0", path = "/foo/asdf/aws-smithy-types" }
            aws-smithy-http = { version = "0.35.0", path = "/foo/asdf/aws-smithy-http", features = ["test-util"] }
            something-else = "0.1"
            "#
        );
    }
}
