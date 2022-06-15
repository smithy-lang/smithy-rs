/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::{bail, Result};
use clap::Parser;
use smithy_rs_tool_common::package::{PackageCategory, SDK_PREFIX};
use smithy_rs_tool_common::versions_manifest::VersionsManifest;
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
#[allow(clippy::enum_variant_names)] // Want the "use" prefix in the CLI subcommand names for clarity
enum Args {
    /// Revise crates to use paths in dependencies
    UsePathDependencies {
        /// Root SDK path the path dependencies will be based off of
        #[clap(long)]
        sdk_path: PathBuf,
        /// Path(s) to recursively update Cargo.toml files in
        #[clap()]
        crate_paths: Vec<PathBuf>,
    },
    /// Revise crates to use version numbers in dependencies
    UseVersionDependencies {
        /// Path to a `versions.toml` file with crate versions to use
        #[clap(long)]
        versions_toml: PathBuf,
        /// Path(s) to recursively update Cargo.toml files in
        #[clap()]
        crate_paths: Vec<PathBuf>,
    },
    /// Revise crates to use version numbers AND paths in dependencies
    UsePathAndVersionDependencies {
        /// Root SDK path the path dependencies will be based off of
        #[clap(long)]
        sdk_path: PathBuf,
        /// Path to a `versions.toml` file with crate versions to use
        #[clap(long)]
        versions_toml: PathBuf,
        /// Path(s) to recursively update Cargo.toml files in
        #[clap()]
        crate_paths: Vec<PathBuf>,
    },
}

impl Args {
    fn crate_paths(&self) -> &[PathBuf] {
        match self {
            Self::UsePathDependencies { crate_paths, .. } => crate_paths,
            Self::UseVersionDependencies { crate_paths, .. } => crate_paths,
            Self::UsePathAndVersionDependencies { crate_paths, .. } => crate_paths,
        }
    }

    fn validate(self) -> Result<Self> {
        if self.crate_paths().is_empty() {
            bail!("Must provide at least one crate path to recursively update");
        }
        Ok(self)
    }
}

struct DependencyContext<'a> {
    sdk_path: Option<&'a Path>,
    versions_manifest: Option<VersionsManifest>,
}

fn main() -> Result<()> {
    let args = Args::parse().validate()?;
    let dependency_context = match &args {
        Args::UsePathDependencies { sdk_path, .. } => DependencyContext {
            sdk_path: Some(sdk_path),
            versions_manifest: None,
        },
        Args::UseVersionDependencies { versions_toml, .. } => DependencyContext {
            sdk_path: None,
            versions_manifest: Some(VersionsManifest::from_file(&versions_toml)?),
        },
        Args::UsePathAndVersionDependencies {
            sdk_path,
            versions_toml,
            ..
        } => DependencyContext {
            sdk_path: Some(sdk_path),
            versions_manifest: Some(VersionsManifest::from_file(&versions_toml)?),
        },
    };

    let start_time = Instant::now();
    let mut manifest_paths = Vec::new();
    for crate_path in args.crate_paths() {
        discover_manifests(&mut manifest_paths, crate_path)?;
    }

    for manifest_path in manifest_paths {
        update_manifest(&manifest_path, &dependency_context)?;
    }

    println!("Finished in {:?}", start_time.elapsed());
    Ok(())
}

fn update_manifest(
    manifest_path: &Path,
    dependency_context: &DependencyContext,
) -> anyhow::Result<()> {
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
            changed =
                update_dependencies(dependencies.as_table_mut().unwrap(), dependency_context)?
                    || changed;
        }
    }

    if changed {
        fs::write(manifest_path, &toml::to_vec(&metadata)?)?;
    }

    Ok(())
}

fn update_dependencies(
    dependencies: &mut Table,
    dependency_context: &DependencyContext,
) -> Result<bool> {
    let mut changed = false;
    for (key, value) in dependencies.iter_mut() {
        let category = PackageCategory::from_package_name(key);
        if !matches!(category, PackageCategory::Unknown) {
            if !value.is_table() {
                *value = Value::Table(Table::new());
            }
            update_dependency_value(key, value.as_table_mut().unwrap(), dependency_context)?;
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

fn update_dependency_value(
    crate_name: &str,
    value: &mut Table,
    dependency_context: &DependencyContext,
) -> Result<()> {
    // Remove keys that will be replaced
    value.remove("git");
    value.remove("branch");
    value.remove("version");
    value.remove("path");

    // Set the `path` if one was given
    if let Some(path) = &dependency_context.sdk_path {
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
    if let Some(manifest) = &dependency_context.versions_manifest {
        if let Some(crate_metadata) = manifest.crates.get(crate_name) {
            value.insert(
                "version".to_string(),
                Value::String(crate_metadata.version.clone()),
            );
        } else {
            bail!(
                "Crate `{}` was missing from the `versions.toml`",
                crate_name
            );
        }
    }

    Ok(())
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
    use crate::{update_manifest, DependencyContext};
    use pretty_assertions::assert_eq;
    use smithy_rs_tool_common::package::PackageCategory;
    use smithy_rs_tool_common::versions_manifest::{CrateVersion, VersionsManifest};
    use std::path::PathBuf;
    use toml::Value;

    fn versions_toml_for(crates: &[(&str, &str)]) -> VersionsManifest {
        VersionsManifest {
            smithy_rs_revision: "doesntmatter".into(),
            aws_doc_sdk_examples_revision: "doesntmatter".into(),
            crates: crates
                .iter()
                .map(|&(name, version)| {
                    (
                        name.to_string(),
                        CrateVersion {
                            category: PackageCategory::from_package_name(name),
                            version: version.into(),
                            source_hash: "doesntmatter".into(),
                            model_hash: None,
                        },
                    )
                })
                .collect(),
            release: None,
        }
    }

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
    fn test_with_context(context: DependencyContext, expected: &[u8]) {
        let manifest_file = tempfile::NamedTempFile::new().unwrap();
        let manifest_path = manifest_file.into_temp_path();
        std::fs::write(&manifest_path, TEST_MANIFEST).unwrap();

        update_manifest(&manifest_path, &context).expect("success");

        let actual = toml::from_slice(&std::fs::read(&manifest_path).expect("read tmp file"))
            .expect("valid toml");
        let expected: Value = toml::from_slice(expected).unwrap();
        assert_eq!(expected, actual);
    }

    #[test]
    fn update_dependencies_with_versions() {
        test_with_context(
            DependencyContext {
                sdk_path: None,
                versions_manifest: Some(versions_toml_for(&[
                    ("aws-config", "0.5.0"),
                    ("aws-sdk-s3", "0.13.0"),
                    ("aws-smithy-types", "0.10.0"),
                    ("aws-smithy-http", "0.9.0"),
                ])),
            },
            br#"
            [package]
            name = "test"
            version = "0.1.0"

            [dependencies]
            aws-config = { version = "0.5.0" }
            aws-sdk-s3 = { version = "0.13.0" }
            aws-smithy-types = { version = "0.10.0" }
            aws-smithy-http = { version = "0.9.0", features = ["test-util"] }
            something-else = "0.1"
            "#,
        );
    }

    #[test]
    fn update_dependencies_with_paths() {
        test_with_context(
            DependencyContext {
                sdk_path: Some(&PathBuf::from("/foo/asdf/")),
                versions_manifest: None,
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
        test_with_context(
            DependencyContext {
                sdk_path: Some(&PathBuf::from("/foo/asdf/")),
                versions_manifest: Some(versions_toml_for(&[
                    ("aws-config", "0.5.0"),
                    ("aws-sdk-s3", "0.13.0"),
                    ("aws-smithy-types", "0.10.0"),
                    ("aws-smithy-http", "0.9.0"),
                ])),
            },
            br#"
            [package]
            name = "test"
            version = "0.1.0"

            [dependencies]
            aws-config = { version = "0.5.0", path = "/foo/asdf/aws-config" }
            aws-sdk-s3 = { version = "0.13.0", path = "/foo/asdf/s3" }
            aws-smithy-types = { version = "0.10.0", path = "/foo/asdf/aws-smithy-types" }
            aws-smithy-http = { version = "0.9.0", path = "/foo/asdf/aws-smithy-http", features = ["test-util"] }
            something-else = "0.1"
            "#
        );
    }
}
