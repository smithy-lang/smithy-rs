/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Packages, package discovery, and package batching logic.

use crate::fs::Fs;
use crate::sort::dependency_order;
use anyhow::Result;
use semver::Version;
use smithy_rs_tool_common::package::{Package, PackageCategory, PackageHandle, Publish};
use std::error::Error as StdError;
use std::path::PathBuf;
use std::{collections::BTreeMap, path::Path};
use tokio::fs;
use tracing::warn;

/// Batch of packages.
pub type PackageBatch = Vec<Package>;

/// Stats about the packages.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct PackageStats {
    /// Number of Smithy runtime crates
    pub smithy_runtime_crates: usize,
    /// Number of AWS runtime crates
    pub aws_runtime_crates: usize,
    /// Number of AWS service crates
    pub aws_sdk_crates: usize,
}

impl PackageStats {
    pub fn total(&self) -> usize {
        self.smithy_runtime_crates + self.aws_runtime_crates + self.aws_sdk_crates
    }

    fn calculate(batches: &[PackageBatch]) -> PackageStats {
        let mut stats = PackageStats::default();
        for batch in batches {
            for package in batch {
                match package.category {
                    PackageCategory::SmithyRuntime => stats.smithy_runtime_crates += 1,
                    PackageCategory::AwsRuntime => stats.aws_runtime_crates += 1,
                    PackageCategory::AwsSdk => stats.aws_sdk_crates += 1,
                    PackageCategory::Unknown => {
                        warn!("Unrecognized crate: {}", package.handle.name);
                    }
                }
            }
        }
        stats
    }
}

/// Discovers publishable packages in the given directory and returns them as
/// batches that can be published in order.
pub async fn discover_and_validate_package_batches(
    fs: Fs,
    path: impl AsRef<Path>,
) -> Result<(Vec<PackageBatch>, PackageStats)> {
    let packages = discover_packages(fs, path.as_ref())
        .await?
        .into_iter()
        .filter(|package| package.publish == Publish::Allowed)
        .collect::<Vec<Package>>();
    validate_packages(&packages)?;
    let batches = batch_packages(packages)?;
    let stats = PackageStats::calculate(&batches);
    Ok((batches, stats))
}

type BoxError = Box<dyn StdError + Send + Sync + 'static>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid manifest {0:?}")]
    InvalidManifest(PathBuf),
    #[error(
        "Invalid crate version {1} in {0:?}: {2}. NOTE: All local dependencies \
         must have complete version numbers rather than version requirements."
    )]
    InvalidCrateVersion(PathBuf, String, BoxError),
    #[error("{0:?} missing version in dependency {1}")]
    MissingVersion(PathBuf, String),
    #[error("crate {0} has multiple versions: {1} and {2}")]
    MultipleVersions(String, Version, Version),
}

/// Discovers all Cargo.toml files under the given path recursively
#[async_recursion::async_recursion]
pub async fn discover_manifests(path: &Path) -> Result<Vec<PathBuf>> {
    let mut manifests = Vec::new();
    let mut read_dir = fs::read_dir(path).await?;
    while let Some(entry) = read_dir.next_entry().await? {
        let package_path = entry.path();
        if package_path.is_dir() {
            let manifest_path = package_path.join("Cargo.toml");
            if manifest_path.exists() {
                manifests.push(manifest_path);
            }
            manifests.extend(discover_manifests(&package_path).await?.into_iter());
        }
    }
    Ok(manifests)
}

/// Discovers and parses all Cargo.toml files that are packages (as opposed to being exclusively workspaces)
pub async fn discover_packages(fs: Fs, path: &Path) -> Result<Vec<Package>> {
    let manifest_paths = discover_manifests(path).await?;
    read_packages(fs, manifest_paths).await
}

/// Validates that all of the publishable crates use consistent version numbers
/// across all of their local dependencies.
fn validate_packages(packages: &[Package]) -> Result<()> {
    let mut versions: BTreeMap<String, Version> = BTreeMap::new();
    let track_version = &mut |handle: &PackageHandle| -> Result<(), Error> {
        if let Some(version) = versions.get(&handle.name) {
            if version != handle.expect_version() {
                Err(Error::MultipleVersions(
                    (&handle.name).into(),
                    versions[&handle.name].clone(),
                    handle.expect_version().clone(),
                ))
            } else {
                Ok(())
            }
        } else {
            versions.insert(handle.name.clone(), handle.expect_version().clone());
            Ok(())
        }
    };
    for package in packages {
        track_version(&package.handle)?;
        for dependency in &package.local_dependencies {
            track_version(dependency)?;
        }
    }

    Ok(())
}

pub async fn read_packages(fs: Fs, manifest_paths: Vec<PathBuf>) -> Result<Vec<Package>> {
    let mut result = Vec::new();
    for path in &manifest_paths {
        let contents: Vec<u8> = fs.read_file(path).await?;
        if let Some(package) = Package::try_load_manifest(path, &contents)? {
            result.push(package);
        }
    }
    Ok(result)
}

/// Splits the given packages into a list of batches that can be published in order.
/// All of the packages in a given batch can be safely published in parallel.
fn batch_packages(packages: Vec<Package>) -> Result<Vec<PackageBatch>> {
    // Sort packages in order of local dependencies
    let mut packages = dependency_order(packages)?;

    // Discover batches
    let mut batches = Vec::new();
    'outer: while packages.len() > 1 {
        for run in 0..packages.len() {
            let next = &packages[run];
            // If the next package depends on any prior package, then we've discovered the end of the batch
            for index in 0..run {
                let previous = &packages[index];
                if next.locally_depends_on(&previous.handle) {
                    let remaining = packages.split_off(run);
                    let batch = packages;
                    packages = remaining;
                    batches.push(batch);
                    continue 'outer;
                }
            }
        }
        // If the current run is the length of the package vec, then we have exactly one batch left
        break;
    }

    // Push the final batch
    if !packages.is_empty() {
        batches.push(packages);
    }

    // Sort packages within batches so that `--continue-from` work consistently
    for batch in batches.iter_mut() {
        batch.sort();
    }
    Ok(batches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use semver::Version;

    fn package(name: &str, dependencies: &[&str]) -> Package {
        Package::new(
            PackageHandle::new(name, Version::parse("1.0.0").ok()),
            format!("{name}/Cargo.toml"),
            dependencies
                .iter()
                .map(|d| PackageHandle::new(*d, Version::parse("1.0.0").ok()))
                .collect(),
            Publish::Allowed,
        )
    }

    fn fmt_batches(batches: Vec<PackageBatch>) -> String {
        let mut result = String::new();
        for batch in batches {
            result.push_str(
                &batch
                    .iter()
                    .map(|p| p.handle.name.as_str())
                    .collect::<Vec<&str>>()
                    .join(","),
            );
            result.push(';');
        }
        result
    }

    #[test]
    fn test_batch_packages() {
        assert_eq!("", fmt_batches(batch_packages(vec![]).unwrap()));
        assert_eq!(
            "A;",
            fmt_batches(batch_packages(vec![package("A", &[])]).unwrap())
        );
        assert_eq!(
            "A,B;",
            fmt_batches(batch_packages(vec![package("A", &[]), package("B", &[])]).unwrap())
        );
        assert_eq!(
            "A,B;C;",
            fmt_batches(
                batch_packages(vec![
                    package("C", &["A", "B"]),
                    package("B", &[]),
                    package("A", &[]),
                ])
                .unwrap()
            )
        );
        assert_eq!(
            "A,B;C,D,F;E;",
            fmt_batches(
                batch_packages(vec![
                    package("A", &[]),
                    package("B", &[]),
                    package("C", &["A"]),
                    package("D", &["A", "B"]),
                    package("F", &["B"]),
                    package("E", &["C", "D", "F"]),
                ])
                .unwrap()
            )
        );
        assert_eq!(
            "A,F;B;C;E,G;D,H,I;",
            fmt_batches(
                batch_packages(vec![
                    package("F", &[]),
                    package("G", &["C"]),
                    package("I", &["G"]),
                    package("H", &["G"]),
                    package("D", &["B", "C"]),
                    package("E", &["C"]),
                    package("C", &["B"]),
                    package("A", &[]),
                    package("B", &["A"]),
                ])
                .unwrap()
            )
        );
    }

    fn pkg_ver(name: &str, version: &str, dependencies: &[(&str, &str)]) -> Package {
        Package::new(
            PackageHandle::new(name, Some(Version::parse(version).unwrap())),
            format!("{name}/Cargo.toml"),
            dependencies
                .iter()
                .map(|p| PackageHandle::new(p.0, Some(Version::parse(p.1).unwrap())))
                .collect(),
            Publish::Allowed,
        )
    }

    #[test]
    fn test_validate_packages() {
        validate_packages(&vec![
            pkg_ver("A", "1.0.0", &[]),
            pkg_ver("B", "1.1.0", &[]),
            pkg_ver("C", "1.2.0", &[("A", "1.0.0"), ("B", "1.1.0")]),
            pkg_ver("D", "1.3.0", &[("A", "1.0.0")]),
            pkg_ver("F", "1.4.0", &[("B", "1.1.0")]),
            pkg_ver(
                "E",
                "1.5.0",
                &[("C", "1.2.0"), ("D", "1.3.0"), ("F", "1.4.0")],
            ),
        ])
        .expect("success");

        let error = validate_packages(&vec![
            pkg_ver("A", "1.1.0", &[]),
            pkg_ver("B", "1.1.0", &[]),
            pkg_ver("C", "1.2.0", &[("A", "1.1.0"), ("B", "1.1.0")]),
            pkg_ver("D", "1.3.0", &[("A", "1.0.0")]),
            pkg_ver("F", "1.4.0", &[("B", "1.1.0")]),
            pkg_ver(
                "E",
                "1.5.0",
                &[("C", "1.2.0"), ("D", "1.3.0"), ("F", "1.4.0")],
            ),
        ])
        .expect_err("fail");
        assert_eq!(
            "crate A has multiple versions: 1.1.0 and 1.0.0",
            format!("{error}")
        );
    }

    #[test]
    fn test_expected_package_owners_server_crate() {
        let server_packages = vec![
            package("aws-smithy-http-server", &[]),
            package("aws-smithy-http-server-python", &[]),
            package("aws-smithy-http-server-typescript", &[]),
            package("aws-smithy-legacy-http-server", &[]),
        ];
        for pkg in server_packages {
            assert_eq!(&["aws-sdk-rust-ci"], pkg.expected_owners());
        }
    }

    #[test]
    fn test_expected_package_owners_sdk_crate() {
        let sdk_package = package("aws-types", &[]);
        assert_eq!(&["aws-sdk-rust-ci"], sdk_package.expected_owners());
    }

    #[test]
    fn test_expected_package_owners_smithy_runtime_crate() {
        let smithy_runtime_package = package("aws-smithy-types", &[]);
        assert_eq!(
            &["aws-sdk-rust-ci"],
            smithy_runtime_package.expected_owners()
        );
    }
}
