/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Subcommand for fixing manifest dependency version numbers.
//!
//! Finds all of the version numbers for every crate in the repo crate path, and then
//! finds all references to the crates in that path and updates them to have the correct
//! version numbers in addition to the dependency path.

use crate::fs::Fs;
use crate::package::{discover_manifests, parse_version, SemVer};
use crate::SDK_REPO_NAME;
use anyhow::{bail, Context, Result};
use clap::Parser;
use semver::Version;
use smithy_rs_tool_common::ci::running_in_ci;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use toml::value::Table;
use toml::Value;
use tracing::{debug, info};

mod validate;

const AWS_SDK_CRATE_NAME_PREFIX: &str = "aws-sdk-";

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Mode {
    Check,
    Execute,
}

#[derive(Parser, Debug)]
pub struct FixManifestsArgs {
    /// Path containing the manifests to fix. Manifests will be discovered recursively
    #[clap(long)]
    location: PathBuf,
    /// Checks manifests rather than fixing them
    #[clap(long)]
    check: bool,
    /// Disable expected version number validation. This should only be used
    /// when SDK crates are being generated with independent version numbers.
    #[clap(long)]
    disable_version_number_validation: bool,
}

pub async fn subcommand_fix_manifests(
    FixManifestsArgs {
        location,
        check,
        disable_version_number_validation,
    }: &FixManifestsArgs,
) -> Result<()> {
    let mode = match check {
        true => Mode::Check,
        false => Mode::Execute,
    };
    let manifest_paths = discover_manifests(location.into()).await?;
    let mut manifests = read_manifests(Fs::Real, manifest_paths).await?;
    let versions = package_versions(&manifests)?;

    validate::validate_before_fixes(&versions, *disable_version_number_validation)?;
    fix_manifests(Fs::Real, &versions, &mut manifests, mode).await?;
    validate::validate_after_fixes(location).await?;
    info!("Successfully fixed manifests!");
    Ok(())
}

struct Manifest {
    path: PathBuf,
    metadata: toml::Value,
}

impl Manifest {
    /// Returns the `publish` setting for a given crate
    fn publish(&self) -> Result<bool> {
        let value = self.metadata.get("package").and_then(|v| v.get("publish"));
        match value {
            None => Ok(true),
            Some(value) => value
                .as_bool()
                .ok_or(anyhow::Error::msg("unexpected publish setting")),
        }
    }
}

struct Versions(BTreeMap<String, VersionWithMetadata>);
#[derive(Copy, Clone)]
enum FilterType {
    AllCrates,
    PublishedOnly,
}
struct VersionView<'a>(&'a Versions, FilterType);
impl VersionView<'_> {
    fn get(&self, crate_name: &str) -> Option<&Version> {
        let version = match (self.1, self.0 .0.get(crate_name)) {
            (FilterType::AllCrates, version) => version,
            (FilterType::PublishedOnly, v @ Some(VersionWithMetadata { publish: true, .. })) => v,
            _ => None,
        };
        version.map(|v| &v.version)
    }

    fn all_crates(&self) -> Self {
        VersionView(self.0, FilterType::AllCrates)
    }
}

impl Versions {
    fn published(&self) -> VersionView {
        VersionView(self, FilterType::PublishedOnly)
    }

    fn published_crates(&self) -> impl Iterator<Item = (&str, &Version)> + '_ {
        self.0
            .iter()
            .filter(|(_, v)| v.publish)
            .map(|(k, v)| (k.as_str(), &v.version))
    }

    fn get(&self, crate_name: &str) -> Option<&Version> {
        self.0.get(crate_name).map(|v| &v.version)
    }
}

struct VersionWithMetadata {
    version: Version,
    publish: bool,
}

async fn read_manifests(fs: Fs, manifest_paths: Vec<PathBuf>) -> Result<Vec<Manifest>> {
    let mut result = Vec::new();
    for path in manifest_paths {
        let contents = fs.read_file(&path).await?;
        let metadata = toml::from_slice(&contents)
            .with_context(|| format!("failed to load package manifest for {:?}", &path))?;
        result.push(Manifest { path, metadata });
    }
    Ok(result)
}

/// Returns a map of crate name to semver version number
fn package_versions(manifests: &[Manifest]) -> Result<Versions> {
    let mut versions = BTreeMap::new();
    for manifest in manifests {
        // ignore workspace manifests
        let package = match manifest.metadata.get("package") {
            Some(package) => package,
            None => continue,
        };
        let publish = manifest.publish()?;
        let name = package
            .get("name")
            .and_then(|name| name.as_str())
            .ok_or_else(|| {
                anyhow::Error::msg(format!("{:?} is missing a package name", manifest.path))
            })?;
        let version = package
            .get("version")
            .and_then(|name| name.as_str())
            .ok_or_else(|| {
                anyhow::Error::msg(format!("{:?} is missing a package version", manifest.path))
            })?;
        let version = parse_version::<SemVer>(&manifest.path, version)?;
        versions.insert(
            name.into(),
            VersionWithMetadata {
                version: version.try_into_semver().unwrap(),
                publish,
            },
        );
    }
    Ok(Versions(versions))
}

fn fix_dep_set(versions: &VersionView, key: &str, metadata: &mut Value) -> Result<usize> {
    let mut changed = 0;
    if let Some(dependencies) = metadata.as_table_mut().unwrap().get_mut(key) {
        if let Some(dependencies) = dependencies.as_table_mut() {
            for (dep_name, dep) in dependencies.iter_mut() {
                changed += match dep.as_table_mut() {
                    None => {
                        if !dep.is_str() {
                            bail!("unexpected dependency (must be table or string): {:?}", dep)
                        }
                        0
                    }
                    Some(ref mut table) => update_dep(table, dep_name, versions)?,
                };
            }
        }
    }
    Ok(changed)
}

// Update a version of `dep_name` that has a path dependency to be that appearing in `versions`.
//
// While doing so, we will use the tilde version requirement so customers can update patch versions
// automatically. Specifically, we use tilde versions of the form `~major.minor` (e.g. `~1.2`) so
// it can support the range of versions `>=1.2.0, <1.3.0`. See
// https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#tilde-requirements
fn update_dep(table: &mut Table, dep_name: &str, versions: &VersionView) -> Result<usize> {
    if !table.contains_key("path") {
        return Ok(0);
    }
    let package_version = versions
        .get(dep_name)
        .ok_or_else(|| anyhow::Error::msg(format!("version not found for crate {dep_name}")))?;
    let package_version = if dep_name.starts_with(AWS_SDK_CRATE_NAME_PREFIX) {
        // For a crate that depends on an SDK crate (e.g. `aws-config` depending on `aws-sdk-sts`),
        // we do _not_ want to turn the version of the SDK crate into a tilde version because
        // customers may depend on that SDK crate by themselves, causing multiple versions of it
        // to be brought in to their crate graph.
        package_version.to_string()
    } else {
        convert_to_tilde_requirement(&package_version)?
    };
    let previous_version = table.insert(
        "version".into(),
        toml::Value::String(package_version.clone()),
    );
    match previous_version {
        None => Ok(1),
        Some(prev_version) if prev_version.as_str() == Some(&package_version) => Ok(0),
        Some(mismatched_version) => {
            tracing::warn!(expected = ?package_version, actual = ?mismatched_version, "version was set but it did not match");
            Ok(1)
        }
    }
}

// Convert `package_version` into a tilde version requirement
//
// For instance, given `package_version` like `0.12.3`, the function returns `~0.12`.
// The fact that this function takes a `semver::Version` means one can only convert a complete
// semver `x.y.z` into a tilde version requirement, but not those like `0.21.0-alpha.1`.
fn convert_to_tilde_requirement(package_version: &Version) -> Result<String> {
    // `package_version` is from the `semver` crate which requires versions to have 3 components,
    // major, minor, and patch. So it is safe to assume its string value follows that format.
    let package_version = package_version.to_string();
    let package_version = match package_version.rfind('.') {
        // Here, we're interested in the `major.minor` part.
        Some(index) => &package_version[0..index],
        None => bail!("{} did not have any dots in it", package_version),
    };

    Ok("~".to_string() + package_version)
}

fn fix_dep_sets(versions: &VersionView, metadata: &mut toml::Value) -> Result<usize> {
    let mut changed = fix_dep_set(versions, "dependencies", metadata)?;
    // allow dev dependencies to be unpublished
    changed += fix_dep_set(&versions.all_crates(), "dev-dependencies", metadata)?;
    changed += fix_dep_set(versions, "build-dependencies", metadata)?;
    Ok(changed)
}

fn is_example_manifest(manifest_path: impl AsRef<Path>) -> bool {
    // Examine parent directories until either `examples/` or `aws-sdk-rust/` is found
    let mut path = manifest_path.as_ref();
    while let Some(parent) = path.parent() {
        path = parent;
        if path.file_name() == Some(OsStr::new("examples")) {
            return true;
        } else if path.file_name() == Some(OsStr::new(SDK_REPO_NAME)) {
            break;
        }
    }
    false
}

fn conditionally_disallow_publish(
    manifest_path: &Path,
    metadata: &mut toml::Value,
) -> Result<bool> {
    let is_github_actions = running_in_ci();
    let is_example = is_example_manifest(manifest_path);

    // Safe-guard to prevent accidental publish to crates.io. Add some friction
    // to publishing from a local development machine by detecting that the tool
    // is not being run from CI, and disallow publish in that case. Also disallow
    // publishing of examples.
    if !is_github_actions || is_example {
        if let Some(value) = set_publish_false(manifest_path, metadata, is_example) {
            return Ok(value);
        }
    }
    Ok(false)
}

fn set_publish_false(manifest_path: &Path, metadata: &mut Value, is_example: bool) -> Option<bool> {
    if let Some(package) = metadata.as_table_mut().unwrap().get_mut("package") {
        info!(
            "Detected {}. Disallowing publish for {:?}.",
            if is_example { "example" } else { "local build" },
            manifest_path,
        );
        package
            .as_table_mut()
            .unwrap()
            .insert("publish".into(), toml::Value::Boolean(false));
        return Some(true);
    }
    None
}

async fn fix_manifests(
    fs: Fs,
    versions: &Versions,
    manifests: &mut Vec<Manifest>,
    mode: Mode,
) -> Result<()> {
    for manifest in manifests {
        let package_changed =
            conditionally_disallow_publish(&manifest.path, &mut manifest.metadata)?;
        let num_deps_changed = fix_manifest(versions, manifest)?;
        if package_changed || num_deps_changed > 0 {
            let contents =
                "# Code generated by software.amazon.smithy.rust.codegen.smithy-rs. DO NOT EDIT.\n"
                    .to_string()
                    + &toml::to_string(&manifest.metadata).with_context(|| {
                        format!("failed to serialize to toml for {:?}", manifest.path)
                    })?;

            match mode {
                Mode::Execute => {
                    fs.write_file(&manifest.path, contents.as_bytes()).await?;
                    info!(
                        "Changed {} dependencies in {:?}.",
                        num_deps_changed, manifest.path
                    );
                }
                Mode::Check => {
                    bail!(
                        "{manifest:?} contained invalid versions",
                        manifest = manifest.path
                    )
                }
            }
        }
    }
    Ok(())
}

fn fix_manifest(versions: &Versions, manifest: &mut Manifest) -> Result<usize> {
    let mut view = versions.published();
    if !manifest.publish()? {
        debug!(package = ?&manifest.path, "package has publishing disabled, allowing unpublished crates to be used");
        view = view.all_crates();
    }
    fix_dep_sets(&view, &mut manifest.metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_versions<'a>(versions: impl Iterator<Item = &'a (&'a str, &'a str, bool)>) -> Versions {
        let map = versions
            .into_iter()
            .map(|(name, version, publish)| {
                let publish = *publish;
                (
                    name.to_string(),
                    VersionWithMetadata {
                        version: Version::parse(&version).unwrap(),
                        publish,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();

        Versions(map)
    }

    #[test]
    fn unpublished_deps_cant_be_deps() {
        let manifest = br#"
            [package]
            name = "test"
            version = "1.2.0"

            [build-dependencies]
            build_something = "1.3"
            local_build_something = { path = "../local_build_something", version = "0.4.0-different" }

            [dev-dependencies]
            dev_something = "1.1"
            local_dev_something = { path = "../local_dev_something" }

            [dependencies]
            something = "1.0"
            local_something = { path = "../local_something" }
        "#;
        let metadata = toml::from_slice(manifest).unwrap();
        let mut manifest = Manifest {
            path: "test".into(),
            metadata,
        };
        let versions = &[
            ("local_build_something", "0.2.0", true),
            ("local_dev_something", "0.1.0", false),
            ("local_something", "1.1.3", false),
        ];
        let versions = make_versions(versions.iter());
        fix_manifest(&versions, &mut manifest).expect_err("depends on unpublished local something");
        set_publish_false(&manifest.path, &mut manifest.metadata, false).unwrap();
        fix_manifest(&versions, &mut manifest)
            .expect("now it will work, the crate isn't published");
    }

    #[test]
    fn test_fix_dep_sets() {
        let manifest = br#"
            [package]
            name = "test"
            version = "1.2.0-preview"

            [build-dependencies]
            build_something = "1.3"
            local_build_something = { path = "../local_build_something", version = "0.4.0-different" }

            [dev-dependencies]
            dev_something = "1.1"
            local_dev_something = { path = "../local_dev_something" }

            [dependencies]
            something = "1.0"
            local_something = { path = "../local_something" }
        "#;
        let metadata = toml::from_slice(manifest).unwrap();
        let mut manifest = Manifest {
            path: "test".into(),
            metadata,
        };
        let versions = &[
            ("local_build_something", "0.2.0", true),
            ("local_dev_something", "0.1.0", false),
            ("local_something", "1.1.3", true),
        ];
        let versions = make_versions(versions.iter());

        fix_dep_sets(&versions.published(), &mut manifest.metadata).expect("success");

        let actual_deps = &manifest.metadata["dependencies"];
        assert_eq!(
            "\
                something = \"1.0\"\n\
                \n\
                [local_something]\n\
                path = \"../local_something\"\n\
                version = \"~1.1\"\n\
            ",
            actual_deps.to_string()
        );

        let actual_dev_deps = &manifest.metadata["dev-dependencies"];
        assert_eq!(
            "\
                dev_something = \"1.1\"\n\
                \n\
                [local_dev_something]\n\
                path = \"../local_dev_something\"\n\
                version = \"~0.1\"\n\
            ",
            actual_dev_deps.to_string()
        );

        let actual_build_deps = &manifest.metadata["build-dependencies"];
        assert_eq!(
            "\
                build_something = \"1.3\"\n\
                \n\
                [local_build_something]\n\
                path = \"../local_build_something\"\n\
                version = \"~0.2\"\n\
            ",
            actual_build_deps.to_string()
        );
    }

    #[test]
    fn sdk_crate_version_should_not_be_turned_into_tilde_requirement() {
        let manifest = br#"
            [package]
            name = "test"
            version = "1.2.0-preview"

            [dependencies]
            aws-sdk-example = { path = "../aws/sdk/example" }
        "#;
        let metadata = toml::from_slice(manifest).unwrap();
        let mut manifest = Manifest {
            path: "test".into(),
            metadata,
        };
        let versions = &[("aws-sdk-example", "0.2.0", true)];
        let versions = make_versions(versions.iter());

        fix_dep_sets(&versions.published(), &mut manifest.metadata).expect("success");

        let actual_deps = &manifest.metadata["dependencies"];
        assert_eq!(
            "\
                [aws-sdk-example]\n\
                path = \"../aws/sdk/example\"\n\
                version = \"0.2.0\"\n\
            ",
            actual_deps.to_string()
        );
    }

    #[test]
    fn test_is_example_manifest() {
        assert!(!is_example_manifest("aws-sdk-rust/sdk/s3/Cargo.toml"));
        assert!(!is_example_manifest(
            "aws-sdk-rust/sdk/aws-config/Cargo.toml"
        ));
        assert!(!is_example_manifest(
            "/path/to/aws-sdk-rust/sdk/aws-config/Cargo.toml"
        ));
        assert!(!is_example_manifest("sdk/aws-config/Cargo.toml"));
        assert!(is_example_manifest("examples/foo/Cargo.toml"));
        assert!(is_example_manifest("examples/foo/bar/Cargo.toml"));
        assert!(is_example_manifest(
            "aws-sdk-rust/examples/foo/bar/Cargo.toml"
        ));
    }
}
