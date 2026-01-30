/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::{bail, Context, Result};
use clap::Parser;
use smithy_rs_tool_common::ci::{is_in_example_dir, is_preview_build};
use smithy_rs_tool_common::package::{PackageCategory, SDK_PREFIX};
use smithy_rs_tool_common::versions_manifest::VersionsManifest;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use toml_edit::{DocumentMut, InlineTable, Item, Table, TableLike, Value};

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

// TODO(https://github.com/smithy-lang/smithy-rs/issues/2810): Remove `SdkPath` and just use a `PathBuf` with the new logic
// This is only around for backwards compatibility for the next release's sync process0
enum SdkPath {
    /// Don't even attempt to resolve the correct relative path to dependencies
    UseDumbLogic(PathBuf),
    /// Resolve the correct relative path to dependencies
    UseNewLogic(PathBuf),
}
impl From<&PathBuf> for SdkPath {
    fn from(value: &PathBuf) -> Self {
        if !value.exists() {
            SdkPath::UseDumbLogic(value.into())
        } else {
            SdkPath::UseNewLogic(value.into())
        }
    }
}

struct DependencyContext {
    sdk_path: Option<SdkPath>,
    versions_manifest: Option<VersionsManifest>,
}

fn main() -> Result<()> {
    let args = Args::parse().validate()?;
    let dependency_context = match &args {
        Args::UsePathDependencies { sdk_path, .. } => DependencyContext {
            sdk_path: Some(sdk_path.into()),
            versions_manifest: None,
        },
        Args::UseVersionDependencies { versions_toml, .. } => DependencyContext {
            sdk_path: None,
            versions_manifest: Some(VersionsManifest::from_file(versions_toml)?),
        },
        Args::UsePathAndVersionDependencies {
            sdk_path,
            versions_toml,
            ..
        } => DependencyContext {
            sdk_path: Some(sdk_path.into()),
            versions_manifest: Some(VersionsManifest::from_file(versions_toml)?),
        },
    };

    // In the case of a preview build we avoid updating the examples directories since
    // we only generate the single preview SDK, so most SDKs referred to in the examples
    // will be missing
    if is_preview_build() && is_in_example_dir(&args.crate_paths()[0]) {
        return Ok(());
    }

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
    println!("Updating {manifest_path:?}...");
    let crate_path = manifest_path.parent().expect("manifest has a parent");

    let mut metadata: DocumentMut = String::from_utf8(
        fs::read(manifest_path).with_context(|| format!("failed to read {manifest_path:?}"))?,
    )
    .with_context(|| format!("{manifest_path:?} has invalid UTF-8"))?
    .parse::<DocumentMut>()
    .with_context(|| format!("failed to parse {manifest_path:?}"))?;
    let mut changed = false;
    for set in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(dependencies) = metadata.get_mut(set) {
            if !dependencies.is_table() {
                bail!("Unexpected non-table value named `{set}` in {manifest_path:?}");
            }
            changed = update_dependencies(
                dependencies.as_table_like_mut().unwrap(),
                dependency_context,
                crate_path,
            )? || changed;
        }
    }

    if changed {
        fs::write(manifest_path, metadata.to_string())?;
    }

    Ok(())
}

fn update_dependencies(
    dependencies: &mut dyn TableLike,
    dependency_context: &DependencyContext,
    crate_path: &Path,
) -> Result<bool> {
    let mut changed = false;
    for (key, value) in dependencies.iter_mut() {
        let crate_name = extract_real_crate_name(&key, value);

        let category = PackageCategory::from_package_name(&crate_name);
        if !matches!(category, PackageCategory::Unknown) {
            let old_value = match value {
                Item::Table(table) => table.clone(),
                Item::Value(Value::InlineTable(inline)) => inline.clone().into_table(),
                _ => Table::new(),
            };
            *value = Item::Value(Value::InlineTable(updated_dependency_value(
                &crate_name,
                old_value,
                dependency_context,
                crate_path,
            )?));
            changed = true;
        }
    }
    Ok(changed)
}

/// Extracts the real name of the underlying crate when the dependency has an alias
fn extract_real_crate_name(key: &toml_edit::KeyMut, value: &Item) -> String {
    match value {
        Item::Value(Value::InlineTable(inline_table)) => {
            if let Some(Value::String(real_package)) = inline_table.get("package") {
                real_package.value()
            } else {
                key.get()
            }
        }
        _ => key.get(),
    }
    .to_string()
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

fn updated_dependency_value(
    dependency_name: &str,
    old_value: Table,
    dependency_context: &DependencyContext,
    crate_path: &Path,
) -> Result<InlineTable> {
    let crate_path = crate_path
        .canonicalize()
        .context("failed to canonicalize crate path")?;
    let mut value = old_value;

    // Remove keys that will be replaced
    value.remove("git");
    value.remove("branch");
    value.remove("version");
    value.remove("path");

    // Set the `path` if one was given
    match &dependency_context.sdk_path {
        Some(SdkPath::UseDumbLogic(sdk_path)) => {
            let crate_path = sdk_path.join(crate_path_name(dependency_name));
            value["path"] = toml_edit::value(
                crate_path
                    .as_os_str()
                    .to_str()
                    .expect("valid utf-8 path")
                    .to_string(),
            );
        }
        Some(SdkPath::UseNewLogic(sdk_path)) => {
            let dependency_path = sdk_path
                .join(crate_path_name(dependency_name))
                .canonicalize()
                .context(format!("failed to canonicalize sdk_path: {sdk_path:#?} with dependency_name: {dependency_name}"))?;
            if let Some(relative_path) = pathdiff::diff_paths(&dependency_path, &crate_path) {
                value["path"] = toml_edit::value(
                    relative_path
                        .as_os_str()
                        .to_str()
                        .expect("valid utf-8 path")
                        .to_string(),
                );
            } else {
                bail!("Failed to create relative path from {crate_path:?} to {dependency_path:?}");
            }
        }
        _ => {}
    }

    // Set the `version` if one was given
    if let Some(manifest) = &dependency_context.versions_manifest {
        if let Some(crate_metadata) = manifest.crates.get(dependency_name) {
            value["version"] = toml_edit::value(crate_metadata.version.clone());
        } else {
            bail!("Crate `{dependency_name}` was missing from the `versions.toml`");
        }
    }

    value.sort_values_by(|a, _, b, _| b.cmp(a));
    Ok(value.into_inline_table())
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
    use crate::{crate_path_name, update_manifest, DependencyContext, SdkPath};
    use pretty_assertions::assert_eq;
    use smithy_rs_tool_common::package::PackageCategory;
    use smithy_rs_tool_common::versions_manifest::{CrateVersion, VersionsManifest};
    use std::path::PathBuf;
    use std::{fs, process};

    fn versions_toml_for(crates: &[(&str, &str)]) -> VersionsManifest {
        VersionsManifest {
            smithy_rs_revision: "doesntmatter".into(),
            aws_doc_sdk_examples_revision: None,
            manual_interventions: Default::default(),
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

# Some comment that should be preserved
[dependencies]
aws-config = "0.4.1"
aws-sdk-s3 = "0.4.1"
aws-smithy-types = "0.34.1"
aws-smithy-http = { version = "0.34.1", features = ["test-util"] }
something-else = { version = "0.1", no-default-features = true }
tokio = { version = "1.18", features = ["net"] }

[dev-dependencies.another-thing]
# some comment
version = "5.0"
# another comment
features = ["foo", "baz"]
"#;

    #[track_caller]
    fn test_with_context(
        crate_path_rel: &str,
        sdk_crates: &[&'static str],
        context: DependencyContext,
        expected: &[u8],
    ) {
        let temp_dir = tempfile::tempdir().unwrap();
        let crate_path = temp_dir.path().join(crate_path_rel);
        fs::create_dir_all(&crate_path).unwrap();

        let manifest_path = crate_path.join("Cargo.toml");
        std::fs::write(&manifest_path, TEST_MANIFEST).unwrap();

        if let Some(SdkPath::UseNewLogic(sdk_path)) = context.sdk_path.as_ref() {
            for sdk_crate in sdk_crates {
                let sdk_crate_path = temp_dir
                    .path()
                    .join(sdk_path)
                    .join(crate_path_name(sdk_crate));
                fs::create_dir_all(sdk_crate_path).unwrap();
            }
        }
        // Assist with debugging when the tests fail
        if let Ok(output) = process::Command::new("find").arg(temp_dir.path()).output() {
            println!(
                "Test directory structure:\n{}",
                String::from_utf8_lossy(&output.stdout)
            );
        }

        let fixed_context = if let Some(SdkPath::UseNewLogic(sdk_path)) = context.sdk_path.as_ref()
        {
            DependencyContext {
                sdk_path: Some(SdkPath::UseNewLogic(temp_dir.path().join(sdk_path))),
                versions_manifest: context.versions_manifest,
            }
        } else {
            context
        };
        update_manifest(&manifest_path, &fixed_context).expect("success");

        let actual =
            String::from_utf8(std::fs::read(&manifest_path).expect("read tmp file")).unwrap();
        let expected = std::str::from_utf8(expected).unwrap();
        assert_eq!(expected, actual);
    }

    #[test]
    fn update_dependencies_with_versions() {
        test_with_context(
            "examples/foo",
            &[],
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

# Some comment that should be preserved
[dependencies]
aws-config = { version = "0.5.0" }
aws-sdk-s3 = { version = "0.13.0" }
aws-smithy-types = { version = "0.10.0" }
aws-smithy-http = { version = "0.9.0", features = ["test-util"] }
something-else = { version = "0.1", no-default-features = true }
tokio = { version = "1.18", features = ["net"] }

[dev-dependencies.another-thing]
# some comment
version = "5.0"
# another comment
features = ["foo", "baz"]
"#,
        );
    }

    #[test]
    fn update_dependencies_with_paths() {
        test_with_context(
            "path/to/test",
            &[
                "aws-config",
                "aws-sdk-s3",
                "aws-smithy-types",
                "aws-smithy-http",
            ],
            DependencyContext {
                sdk_path: Some(SdkPath::UseNewLogic(PathBuf::from("sdk"))),
                versions_manifest: None,
            },
            br#"
[package]
name = "test"
version = "0.1.0"

# Some comment that should be preserved
[dependencies]
aws-config = { path = "../../../sdk/aws-config" }
aws-sdk-s3 = { path = "../../../sdk/s3" }
aws-smithy-types = { path = "../../../sdk/aws-smithy-types" }
aws-smithy-http = { path = "../../../sdk/aws-smithy-http", features = ["test-util"] }
something-else = { version = "0.1", no-default-features = true }
tokio = { version = "1.18", features = ["net"] }

[dev-dependencies.another-thing]
# some comment
version = "5.0"
# another comment
features = ["foo", "baz"]
"#,
        );
    }

    #[test]
    fn update_dependencies_with_paths_dumb_logic() {
        test_with_context(
            "path/to/test",
            &[
                "aws-config",
                "aws-sdk-s3",
                "aws-smithy-types",
                "aws-smithy-http",
            ],
            DependencyContext {
                sdk_path: Some(SdkPath::UseDumbLogic(PathBuf::from("a/dumb/path/to"))),
                versions_manifest: None,
            },
            br#"
[package]
name = "test"
version = "0.1.0"

# Some comment that should be preserved
[dependencies]
aws-config = { path = "a/dumb/path/to/aws-config" }
aws-sdk-s3 = { path = "a/dumb/path/to/s3" }
aws-smithy-types = { path = "a/dumb/path/to/aws-smithy-types" }
aws-smithy-http = { path = "a/dumb/path/to/aws-smithy-http", features = ["test-util"] }
something-else = { version = "0.1", no-default-features = true }
tokio = { version = "1.18", features = ["net"] }

[dev-dependencies.another-thing]
# some comment
version = "5.0"
# another comment
features = ["foo", "baz"]
"#,
        );
    }

    #[test]
    fn update_dependencies_with_versions_and_paths() {
        test_with_context(
            "deep/path/to/test",
            &[
                "aws-config",
                "aws-sdk-s3",
                "aws-smithy-types",
                "aws-smithy-http",
            ],
            DependencyContext {
                sdk_path: Some(SdkPath::UseNewLogic(PathBuf::from("sdk"))),
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

# Some comment that should be preserved
[dependencies]
aws-config = { version = "0.5.0", path = "../../../../sdk/aws-config" }
aws-sdk-s3 = { version = "0.13.0", path = "../../../../sdk/s3" }
aws-smithy-types = { version = "0.10.0", path = "../../../../sdk/aws-smithy-types" }
aws-smithy-http = { version = "0.9.0", path = "../../../../sdk/aws-smithy-http", features = ["test-util"] }
something-else = { version = "0.1", no-default-features = true }
tokio = { version = "1.18", features = ["net"] }

[dev-dependencies.another-thing]
# some comment
version = "5.0"
# another comment
features = ["foo", "baz"]
"#
        );
    }

    #[test]
    fn update_aliased_dependency_with_real_crate_name() {
        let temp_dir = tempfile::tempdir().unwrap();
        let crate_path = temp_dir.path().join("test");
        fs::create_dir_all(&crate_path).unwrap();

        let manifest_path = crate_path.join("Cargo.toml");
        std::fs::write(
            &manifest_path,
            br#"
[package]
name = "test"
version = "0.1.0"

[dependencies]
config = { package = "aws-config", path = "not/a/real/path" }
"#,
        )
        .unwrap();

        let context = DependencyContext {
            sdk_path: None,
            versions_manifest: Some(versions_toml_for(&[
                ("aws-config", "0.5.0"),
                ("config", "1.0.0"),
            ])),
        };

        update_manifest(&manifest_path, &context).expect("success");

        let actual = std::fs::read_to_string(&manifest_path).unwrap();
        let expected = r#"
[package]
name = "test"
version = "0.1.0"

[dependencies]
config = { version = "0.5.0", package = "aws-config" }
"#;
        assert_eq!(expected, actual);
    }
}
