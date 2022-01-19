/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use anyhow::bail;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use structopt::StructOpt;
use toml::value::{Table, Value};

const AWS_CONFIG: &str = "aws-config";
const AWS_RUNTIME_CRATES: &[&str] = &[
    "aws-endpoint",
    "aws-http",
    "aws-hyper",
    "aws-sig-auth",
    "aws-sigv4",
    "aws-types",
];
const SDK_PREFIX: &str = "aws-sdk-";
const SMITHY_PREFIX: &str = "aws-smithy-";

#[derive(StructOpt, Debug)]
#[structopt(
    name = "sdk-versioner",
    about = "CLI tool to recursively update SDK/Smithy crate references in Cargo.toml files"
)]
struct Opt {
    /// Path(s) to recursively update Cargo.toml files in
    #[structopt()]
    crate_paths: Vec<PathBuf>,

    /// SDK version to point to
    #[structopt(long)]
    sdk_version: Option<String>,
    /// Smithy version to point to
    #[structopt(long)]
    smithy_version: Option<String>,

    /// Path to generated SDK to point to
    #[structopt(long)]
    sdk_path: Option<PathBuf>,
}

impl Opt {
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

fn main() -> anyhow::Result<()> {
    let opt = Opt::from_args().validate()?;

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

fn update_manifest(manifest_path: &Path, opt: &Opt) -> anyhow::Result<()> {
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

fn update_dependencies(dependencies: &mut Table, opt: &Opt) -> anyhow::Result<bool> {
    let mut changed = false;
    for (key, value) in dependencies.iter_mut() {
        if is_sdk_or_runtime_crate(key) {
            if !value.is_table() {
                *value = Value::Table(Table::new());
            }
            update_dependency_value(key, value.as_table_mut().unwrap(), opt);
            changed = true;
        }
    }
    Ok(changed)
}

fn is_sdk_crate(name: &str) -> bool {
    name.starts_with(SDK_PREFIX) || name == AWS_CONFIG
}

fn is_sdk_or_runtime_crate(name: &str) -> bool {
    is_sdk_crate(name)
        || name.starts_with(SMITHY_PREFIX)
        || AWS_RUNTIME_CRATES.iter().any(|&k| k == name)
}

fn update_dependency_value(crate_name: &str, value: &mut Table, opt: &Opt) {
    let is_sdk_crate = is_sdk_crate(crate_name);

    // Remove keys that will be replaced
    value.remove("version");
    value.remove("path");

    // Set the `path` if one was given
    if let Some(path) = &opt.sdk_path {
        let crate_path = if is_sdk_crate && crate_name != AWS_CONFIG {
            path.join(&crate_name[SDK_PREFIX.len()..])
        } else {
            path.join(crate_name)
        };
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
    use crate::{update_manifest, Opt};
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
    fn test_with_opt(opt: Opt, expected: &[u8]) {
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
            Opt {
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
            Opt {
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
            Opt {
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
