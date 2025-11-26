/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::RUST_SDK_CI_OWNER;
use anyhow::{Context, Result};
use cargo_toml::{Dependency, DepsSet, Manifest};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fmt, fs,
    path::{Path, PathBuf},
};

pub const SMITHY_PREFIX: &str = "aws-smithy-";
pub const SDK_PREFIX: &str = "aws-sdk-";

// AWS SDK High Level Libraries
pub(crate) static AWS_SDK_HLL_PACKAGES: &[&str] = &["aws-sdk-cloudfront-url-signer"];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Deserialize, Serialize)]
pub enum PackageCategory {
    SmithyRuntime,
    AwsRuntime,
    AwsSdk,
    AwsSdkHll,
    Unknown,
}

impl PackageCategory {
    /// Returns true if the category is `AwsRuntime` or `AwsSdk`
    pub fn is_sdk(&self) -> bool {
        matches!(
            self,
            PackageCategory::AwsRuntime | PackageCategory::AwsSdk | PackageCategory::AwsSdkHll
        )
    }

    /// Categorizes a package based on its name
    pub fn from_package_name(name: &str) -> PackageCategory {
        if AWS_SDK_HLL_PACKAGES.contains(&name) {
            PackageCategory::AwsSdkHll
        } else if name.starts_with(SMITHY_PREFIX) {
            PackageCategory::SmithyRuntime
        } else if name.starts_with(SDK_PREFIX) {
            PackageCategory::AwsSdk
        } else if name.starts_with("aws-") {
            PackageCategory::AwsRuntime
        } else {
            PackageCategory::Unknown
        }
    }
}

/// Enum to denote whether a package we have control over publishing is stable or not
///
/// If a package is a third-party one and we cannot publish it, then it is considered as `Unstable`.
/// In general, tooling cares about crates that we have control over publishing, so the third-party
/// crates being marked as `Unstable` does not affect the integrity of tooling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackageStability {
    Stable,
    Unstable,
}

/// Information required to identify a package (crate).
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct PackageHandle {
    pub name: String,
    pub version: Option<Version>,
}

impl PackageHandle {
    pub fn new(name: impl Into<String>, version: Option<Version>) -> Self {
        Self {
            name: name.into(),
            version,
        }
    }

    /// Returns the expected owners of the crate.
    pub fn expected_owners(&self) -> &[&str] {
        expected_owners()
    }

    /// Returns the version number or panics
    pub fn expect_version(&self) -> &Version {
        if let Some(version) = &self.version {
            version
        } else {
            panic!("Crate version number required for {}", self.name)
        }
    }
}

impl fmt::Display for PackageHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.version {
            Some(version) => write!(f, "{}-{version}", self.name),
            _ => f.write_str(&self.name),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum Publish {
    Allowed,
    NotAllowed,
}

/// Represents a crate (called Package since crate is a reserved word).
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub struct Package {
    /// Package name and version information
    pub handle: PackageHandle,
    /// Package category (Generated, SmithyRuntime, AwsRuntime, etc.)
    pub category: PackageCategory,
    /// Location to the crate on the current file system
    pub crate_path: PathBuf,
    /// Location to the crate manifest on the current file system
    pub manifest_path: PathBuf,
    /// Dependencies used by this package
    pub local_dependencies: BTreeSet<PackageHandle>,
    /// Whether or not the package should be published
    pub publish: Publish,
}

impl Package {
    pub fn new(
        handle: PackageHandle,
        manifest_path: impl Into<PathBuf>,
        local_dependencies: BTreeSet<PackageHandle>,
        publish: Publish,
    ) -> Self {
        let manifest_path = manifest_path.into();
        let category = PackageCategory::from_package_name(&handle.name);
        Self {
            handle,
            category,
            crate_path: manifest_path.parent().unwrap().into(),
            manifest_path,
            local_dependencies,
            publish,
        }
    }

    /// Try to load a package from the given path.
    pub fn try_load_path(path: impl AsRef<Path>) -> Result<Option<Package>> {
        let path = path.as_ref();
        let manifest_path = path.join("Cargo.toml");
        if path.is_dir() && manifest_path.exists() {
            let manifest = fs::read(&manifest_path)
                .context("failed to read manifest")
                .with_context(|| format!("{manifest_path:?}"))?;
            Self::try_load_manifest(manifest_path, &manifest)
        } else {
            Ok(None)
        }
    }

    /// Returns `Ok(None)` when the Cargo.toml is a workspace rather than a package
    pub fn try_load_manifest(
        manifest_path: impl AsRef<Path>,
        manifest: &[u8],
    ) -> Result<Option<Package>> {
        let manifest_path = manifest_path.as_ref();
        let mut manifest = Manifest::from_slice(manifest)
            .with_context(|| format!("failed to load package manifest for {manifest_path:?}"))?;
        manifest.complete_from_path(manifest_path)?;
        if let Some(package) = manifest.package {
            let name = package.name;
            let version = parse_version(manifest_path, &package.version.unwrap())?;
            let handle = PackageHandle {
                name,
                version: Some(version),
            };
            let publish = match package.publish.unwrap() {
                cargo_toml::Publish::Flag(true) => Publish::Allowed,
                _ => Publish::NotAllowed,
            };

            let mut local_dependencies = BTreeSet::new();
            local_dependencies.extend(read_dependencies(manifest_path, &manifest.dependencies)?);
            local_dependencies.extend(read_dependencies(
                manifest_path,
                &manifest.dev_dependencies,
            )?);
            local_dependencies.extend(read_dependencies(
                manifest_path,
                &manifest.build_dependencies,
            )?);
            Ok(Some(Package::new(
                handle,
                manifest_path,
                local_dependencies,
                publish,
            )))
        } else {
            Ok(None)
        }
    }

    /// Returns `true` if this package depends on `other`
    pub fn locally_depends_on(&self, other: &PackageHandle) -> bool {
        self.local_dependencies.contains(other)
    }

    /// Returns the expected owners of the crate.
    pub fn expected_owners(&self) -> &[&str] {
        expected_owners()
    }
}

fn expected_owners() -> &'static [&'static str] {
    &[RUST_SDK_CI_OWNER]
}

/// Parses a semver version number and adds additional error context when parsing fails.
pub fn parse_version(manifest_path: &Path, version: &str) -> Result<Version> {
    Version::parse(version)
        .with_context(|| format!("Invalid crate version {version} in {manifest_path:?}."))
}

fn read_dependencies(path: &Path, dependencies: &DepsSet) -> Result<Vec<PackageHandle>> {
    let mut result = Vec::new();
    for (name, metadata) in dependencies {
        match metadata {
            Dependency::Simple(_) => {}
            Dependency::Detailed(detailed) => {
                if detailed.path.is_some() {
                    let version = detailed
                        .version
                        .as_ref()
                        .map(|version| parse_version(path, version))
                        .transpose()?;
                    result.push(PackageHandle::new(name, version));
                }
            }
            Dependency::Inherited(_) => panic!("workspace deps are unsupported"),
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use semver::Version;

    fn version(version: &str) -> Option<Version> {
        Some(Version::parse(version).unwrap())
    }

    #[test]
    fn try_load_manifest_success() {
        let manifest = br#"
            [package]
            name = "test"
            version = "1.2.0-preview"

            [build-dependencies]
            build_something = "1.3"
            local_build_something = { version = "0.2.0", path = "../local_build_something" }

            [dev-dependencies]
            dev_something = "1.1"
            local_dev_something = { version = "0.1.0", path = "../local_dev_something" }

            [dependencies]
            something = "1.0"
            local_something = { version = "1.1.3", path = "../local_something" }
        "#;
        let path: PathBuf = "test/Cargo.toml".into();

        let package = Package::try_load_manifest(&path, manifest)
            .expect("parse success")
            .expect("is a package");
        assert_eq!("test", package.handle.name);
        assert_eq!(version("1.2.0-preview"), package.handle.version);

        let mut expected = BTreeSet::new();
        expected.insert(PackageHandle::new(
            "local_build_something",
            version("0.2.0"),
        ));
        expected.insert(PackageHandle::new("local_dev_something", version("0.1.0")));
        expected.insert(PackageHandle::new("local_something", version("1.1.3")));
        assert_eq!(expected, package.local_dependencies);
    }

    #[test]
    fn try_load_manifest_version_requirement_invalid() {
        let manifest = br#"
            [package]
            name = "test"
            version = "1.2.0-preview"

            [dependencies]
            local_something = { version = "1.0", path = "../local_something" }
        "#;
        let path: PathBuf = "test/Cargo.toml".into();

        let error = format!(
            "{}",
            Package::try_load_manifest(&path, manifest).expect_err("should fail")
        );
        assert!(
            error.contains("Invalid crate version"),
            "'{}' should contain 'Invalid crate version'",
            error
        );
    }

    fn package(name: &str, dependencies: &[&str]) -> Package {
        Package::new(
            PackageHandle::new(name, version("1.0.0")),
            format!("{}/Cargo.toml", name),
            dependencies
                .iter()
                .map(|d| PackageHandle::new(*d, version("1.0.0")))
                .collect(),
            Publish::Allowed,
        )
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
