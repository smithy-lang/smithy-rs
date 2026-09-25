/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::retry::{run_with_retry_sync, ErrorClass};
use anyhow::{anyhow, Context, Error, Result};
use crates_index::Crate;
use reqwest::StatusCode;
use semver::Version;
use serde::Deserialize;
use std::fmt;
use std::{collections::HashMap, time::Duration};
use std::{fs, path::Path};

/// The dependency section of a published manifest that a dependency was declared in.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum PublishedDependencyKind {
    /// A normal dependency, i.e. `[dependencies]`.
    Normal,
    /// A dev dependency, i.e. `[dev-dependencies]`.
    Dev,
    /// A build dependency, i.e. `[build-dependencies]`.
    Build,
}

impl PublishedDependencyKind {
    /// The name that Cargo and the crates.io index use for this section.
    pub fn as_str(&self) -> &'static str {
        match self {
            PublishedDependencyKind::Normal => "normal",
            PublishedDependencyKind::Dev => "dev",
            PublishedDependencyKind::Build => "build",
        }
    }

    fn from_index_name(name: &str) -> Result<Self> {
        match name {
            "normal" => Ok(PublishedDependencyKind::Normal),
            "dev" => Ok(PublishedDependencyKind::Dev),
            "build" => Ok(PublishedDependencyKind::Build),
            other => Err(anyhow!(
                "unrecognized dependency kind '{other}' (expected one of: normal, dev, build)"
            )),
        }
    }
}

impl fmt::Display for PublishedDependencyKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<crates_index::DependencyKind> for PublishedDependencyKind {
    fn from(value: crates_index::DependencyKind) -> Self {
        match value {
            crates_index::DependencyKind::Normal => PublishedDependencyKind::Normal,
            crates_index::DependencyKind::Dev => PublishedDependencyKind::Dev,
            crates_index::DependencyKind::Build => PublishedDependencyKind::Build,
        }
    }
}

/// A dependency declared by a published crate version, as recorded in the crates.io index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishedDependency {
    /// The dependency key/import name recorded in the published manifest.
    ///
    /// For a renamed dependency such as
    /// `some-crate-065 = { package = "some-crate", version = "0.65" }`,
    /// this is the rename (`some-crate-065`).
    pub alias: String,
    /// The actual package name: the value of `package = ` when renamed, otherwise the alias.
    pub package: String,
    /// The Cargo version requirement, for example `^0.63.0`.
    pub requirement: String,
    /// The dependency section this dependency was declared in.
    pub kind: PublishedDependencyKind,
    /// The target (`cfg(...)` expression or target triple) this dependency is declared under.
    pub target: Option<String>,
    /// True if the dependency is optional.
    pub optional: bool,
}

/// A single published version of a crate, including its recorded dependency metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishedCrateVersion {
    /// The published version number.
    pub version: String,
    /// True if this version has been yanked. A yanked version is still published,
    /// and its version number cannot be reused.
    pub yanked: bool,
    /// Dependencies recorded for this published version.
    pub dependencies: Vec<PublishedDependency>,
}

impl PublishedCrateVersion {
    /// Creates a non-yanked published version that has no recorded dependency metadata.
    pub fn new(version: impl Into<String>) -> Self {
        Self {
            version: version.into(),
            yanked: false,
            dependencies: Vec::new(),
        }
    }
}

pub struct CratesIndex(Inner);

enum Inner {
    Fake(FakeIndex),
    Real(crates_index::SparseIndex),
}

impl CratesIndex {
    /// Returns a real sparse crates.io index.
    pub fn real() -> Result<Self> {
        Ok(Self(Inner::Real(
            crates_index::SparseIndex::new_cargo_default()
                .context("failed to initialize the sparse crates.io index")?,
        )))
    }

    /// Returns a fake crates.io index from file, panicking if loading fails.
    pub fn fake(path: impl AsRef<Path>) -> Self {
        Self(Inner::Fake(FakeIndex::from_file(path)))
    }

    /// Returns a fake crates.io index from a hashmap
    pub fn fake_from_map(versions: HashMap<String, Vec<String>>) -> Self {
        Self(Inner::Fake(FakeIndex {
            crates: versions
                .into_iter()
                .map(|(crate_name, versions)| {
                    (
                        crate_name,
                        versions
                            .into_iter()
                            .map(PublishedCrateVersion::new)
                            .collect(),
                    )
                })
                .collect(),
        }))
    }

    /// Retrieves the published versions for the given crate name, with the dependency
    /// metadata recorded for each of those versions.
    pub fn published_crate_versions(&self, crate_name: &str) -> Result<Vec<PublishedCrateVersion>> {
        match &self.0 {
            Inner::Fake(index) => Ok(index.crates.get(crate_name).cloned().unwrap_or_default()),
            Inner::Real(index) => Ok(run_with_retry_sync(
                "retrieve published versions",
                3,
                Duration::from_secs(1),
                || published_crate_versions(index, crate_name),
                |_err| ErrorClass::Retry,
            )?),
        }
    }

    /// Retrieves the published version numbers for the given crate name.
    pub fn published_versions(&self, crate_name: &str) -> Result<Vec<String>> {
        Ok(self
            .published_crate_versions(crate_name)?
            .into_iter()
            .map(|version| version.version)
            .collect())
    }
}

pub fn is_published(index: &CratesIndex, crate_name: &str, version: &Version) -> Result<bool> {
    let crate_name = crate_name.to_string();
    let versions = index.published_versions(&crate_name)?;
    Ok(versions.contains(&version.to_string()))
}

fn published_crate_versions(
    index: &crates_index::SparseIndex,
    crate_name: &str,
) -> Result<Vec<PublishedCrateVersion>> {
    let url = index
        .crate_url(crate_name)
        .expect("crate name is not empty string");
    let crate_meta: Option<Crate> = reqwest::blocking::get(url)
        .map_err(Error::from)
        .and_then(|response| {
            let status = response.status();
            response.bytes().map(|b| (status, b)).map_err(Error::from)
        })
        .and_then(|(status, bytes)| match status {
            status if status.is_success() => {
                Crate::from_slice(&bytes).map_err(Error::from).map(Some)
            }
            StatusCode::NOT_FOUND => Ok(None),
            status => {
                let body = String::from_utf8_lossy(&bytes);
                Err(anyhow!(
                    "request to crates.io index failed ({status}):\n{body}"
                ))
            }
        })
        .with_context(|| format!("failed to retrieve crates.io metadata for {crate_name}"))?;
    Ok(crate_meta
        .map(|meta| meta.versions().iter().map(convert_version).collect())
        .unwrap_or_default())
}

fn convert_version(version: &crates_index::Version) -> PublishedCrateVersion {
    PublishedCrateVersion {
        version: version.version().to_string(),
        yanked: version.is_yanked(),
        dependencies: version
            .dependencies()
            .iter()
            .map(convert_dependency)
            .collect(),
    }
}

fn convert_dependency(dependency: &crates_index::Dependency) -> PublishedDependency {
    PublishedDependency {
        // `name` is the manifest key (the rename, when renamed), and `crate_name`
        // is the package actually depended on.
        alias: dependency.name().to_string(),
        package: dependency.crate_name().to_string(),
        requirement: dependency.requirement().to_string(),
        kind: dependency.kind().into(),
        target: dependency.target().map(ToString::to_string),
        optional: dependency.is_optional(),
    }
}

/// Fake crates.io index for testing
#[derive(Debug)]
pub struct FakeIndex {
    crates: HashMap<String, Vec<PublishedCrateVersion>>,
}

/// On-disk representation of a fake index.
///
/// The `[crates]` table maps crate name to published version numbers, and is the only
/// required content. The optional `[crate_version_metadata]` table attaches yanked state
/// and dependency metadata to individual published versions:
///
/// ```toml
/// [crates]
/// aws-config = ["1.12.0"]
/// aws-smithy-json = ["0.63.0"]
///
/// [crate_version_metadata."aws-config@1.12.0"]
/// yanked = false
/// dependencies = [
///   { alias = "aws-smithy-json", requirement = "^0.63.0", kind = "normal" },
/// ]
/// ```
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FakeIndexFile {
    crates: HashMap<String, Vec<String>>,
    #[serde(default)]
    crate_version_metadata: HashMap<String, FakeVersionMetadata>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FakeVersionMetadata {
    #[serde(default)]
    yanked: bool,
    #[serde(default)]
    dependencies: Vec<FakeDependency>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FakeDependency {
    /// The manifest key for this dependency.
    alias: String,
    /// The package depended on; defaults to `alias` when the dependency isn't renamed.
    #[serde(default)]
    package: Option<String>,
    requirement: String,
    /// One of `normal`, `dev`, or `build`. Defaults to `normal`, matching the index default.
    #[serde(default)]
    kind: Option<String>,
    /// Target expression. An empty or absent value means "not target specific".
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    optional: bool,
}

impl FakeIndex {
    fn from_file(path: impl AsRef<Path>) -> FakeIndex {
        Self::try_from_file(path.as_ref())
            .with_context(|| {
                format!(
                    "failed to load fake crates.io index from {}",
                    path.as_ref().display()
                )
            })
            .unwrap()
    }

    fn try_from_file(path: &Path) -> Result<FakeIndex> {
        let bytes = fs::read(path).context("failed to read fake index")?;
        let parsed: FakeIndexFile =
            toml::from_slice(&bytes).context("failed to parse fake index")?;

        let mut crates: HashMap<String, Vec<PublishedCrateVersion>> = parsed
            .crates
            .into_iter()
            .map(|(crate_name, versions)| {
                (
                    crate_name,
                    versions
                        .into_iter()
                        .map(PublishedCrateVersion::new)
                        .collect(),
                )
            })
            .collect();

        for (key, metadata) in parsed.crate_version_metadata {
            let (crate_name, version) = key.split_once('@').ok_or_else(|| {
                anyhow!("crate_version_metadata key '{key}' must have the form 'name@version'")
            })?;
            let published = crates
                .get_mut(crate_name)
                .and_then(|versions| versions.iter_mut().find(|v| v.version == version))
                .ok_or_else(|| {
                    anyhow!(
                        "crate_version_metadata key '{key}' does not refer to a version \
                         listed in the [crates] table"
                    )
                })?;
            published.yanked = metadata.yanked;
            published.dependencies = metadata
                .dependencies
                .into_iter()
                .map(|dependency| {
                    let kind = match &dependency.kind {
                        Some(kind) => PublishedDependencyKind::from_index_name(kind)
                            .with_context(|| format!("in crate_version_metadata '{key}'"))?,
                        None => PublishedDependencyKind::Normal,
                    };
                    Ok(PublishedDependency {
                        package: dependency
                            .package
                            .unwrap_or_else(|| dependency.alias.clone()),
                        alias: dependency.alias,
                        requirement: dependency.requirement,
                        kind,
                        target: dependency.target.filter(|target| !target.is_empty()),
                        optional: dependency.optional,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
        }

        Ok(FakeIndex { crates })
    }
}

#[cfg(test)]
mod test {
    use super::{
        is_published, CratesIndex, PublishedCrateVersion, PublishedDependency,
        PublishedDependencyKind,
    };
    use semver::Version;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn fake_index_from_str(contents: &str) -> (tempfile::TempDir, CratesIndex) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.toml");
        std::fs::write(&path, contents).unwrap();
        let index = CratesIndex::fake(&path);
        (dir, index)
    }

    /// Ignored test against the real index
    #[ignore]
    #[test]
    fn test_known_published_versions() {
        let index = Arc::new(CratesIndex::real().unwrap());
        let known_published = Version::new(1, 1, 7);
        let known_never_published = Version::new(999, 999, 999);
        assert!(is_published(&index, "aws-smithy-runtime-api", &known_published).unwrap());

        assert!(!is_published(&index, "aws-smithy-runtime-api", &known_never_published).unwrap());
    }

    /// Ignored test against the real index. Verifies that dependency metadata from a real
    /// sparse index record is preserved, including a renamed dependency.
    #[ignore]
    #[test]
    fn test_real_index_dependency_metadata() {
        let index = CratesIndex::real().unwrap();

        // `aws-config 1.12.0` is the version from the incident that motivated this metadata.
        let versions = index.published_crate_versions("aws-config").unwrap();
        let published = versions
            .iter()
            .find(|version| version.version == "1.12.0")
            .expect("aws-config 1.12.0 is published");
        let json = published
            .dependencies
            .iter()
            .find(|dependency| dependency.package == "aws-smithy-json")
            .expect("aws-config depends on aws-smithy-json");
        assert_eq!("aws-smithy-json", json.alias);
        assert_eq!("^0.63.0", json.requirement);
        assert_eq!(PublishedDependencyKind::Normal, json.kind);
        assert_eq!(None, json.target);

        // `aws-smithy-http-server-metrics` declares the same package twice: once as a path
        // dependency, and once renamed as a compatibility pin on an older version line.
        let versions = index
            .published_crate_versions("aws-smithy-http-server-metrics")
            .unwrap();
        let published = versions
            .iter()
            .max_by(|a, b| a.version.cmp(&b.version))
            .expect("aws-smithy-http-server-metrics is published");
        let renamed = published
            .dependencies
            .iter()
            .find(|dependency| dependency.alias == "aws-smithy-http-server-065")
            .expect("the 0.65 compatibility pin is published");
        assert_eq!("aws-smithy-http-server", renamed.package);
        assert!(renamed.requirement.contains("0.65"));
    }

    #[test]
    fn test_against_fake_index() {
        let mut crates = HashMap::new();
        crates.insert(
            "aws-smithy-runtime-api".to_string(),
            vec!["1.1.7".to_string()],
        );
        let index = Arc::new(CratesIndex::fake_from_map(crates));
        let known_published = Version::new(1, 1, 7);
        let known_never_published = Version::new(999, 999, 999);
        assert!(is_published(&index, "aws-smithy-runtime-api", &known_published).unwrap());

        assert!(!is_published(&index, "aws-smithy-runtime-api", &known_never_published).unwrap());
    }

    /// `fake_from_map` has no way to express metadata, so it must produce non-yanked
    /// versions with empty dependency lists.
    #[test]
    fn test_fake_from_map_has_empty_metadata() {
        let mut crates = HashMap::new();
        crates.insert("aws-config".to_string(), vec!["1.12.0".to_string()]);
        let index = CratesIndex::fake_from_map(crates);
        assert_eq!(
            vec![PublishedCrateVersion::new("1.12.0")],
            index.published_crate_versions("aws-config").unwrap()
        );
        assert_eq!(
            vec!["1.12.0".to_string()],
            index.published_versions("aws-config").unwrap()
        );
        assert_eq!(
            Vec::<PublishedCrateVersion>::new(),
            index.published_crate_versions("not-a-crate").unwrap()
        );
    }

    /// Fixtures that predate dependency metadata must keep working unchanged.
    #[test]
    fn test_fake_index_file_without_metadata() {
        let (_dir, index) = fake_index_from_str(
            r#"
            [crates]
            aws-config = ["1.0.0", "1.12.0"]
            aws-smithy-json = ["0.63.0"]
            "#,
        );
        assert_eq!(
            vec!["1.0.0".to_string(), "1.12.0".to_string()],
            index.published_versions("aws-config").unwrap()
        );
        assert_eq!(
            vec![
                PublishedCrateVersion::new("1.0.0"),
                PublishedCrateVersion::new("1.12.0")
            ],
            index.published_crate_versions("aws-config").unwrap()
        );
    }

    #[test]
    fn test_fake_index_file_with_metadata() {
        let (_dir, index) = fake_index_from_str(
            r#"
            [crates]
            aws-config = ["1.11.0", "1.12.0"]

            [crate_version_metadata."aws-config@1.12.0"]
            yanked = true
            dependencies = [
              { alias = "aws-smithy-json", requirement = "^0.63.0" },
              { alias = "aws-smithy-http-server-065", package = "aws-smithy-http-server", requirement = "^0.65", kind = "normal" },
              { alias = "aws-smithy-async", requirement = "^1.2.0", kind = "dev" },
              { alias = "build-dep", requirement = "^1.0.0", kind = "build", optional = true },
              { alias = "windows-only", requirement = "^0.5.0", target = "cfg(windows)" },
              { alias = "explicitly-not-targeted", requirement = "^0.5.0", target = "" },
            ]
            "#,
        );

        let versions = index.published_crate_versions("aws-config").unwrap();
        assert_eq!(
            vec!["1.11.0".to_string(), "1.12.0".to_string()],
            index.published_versions("aws-config").unwrap()
        );

        // The version without metadata keeps the defaults.
        assert_eq!(PublishedCrateVersion::new("1.11.0"), versions[0]);

        let published = &versions[1];
        assert!(published.yanked);
        assert_eq!(
            vec![
                PublishedDependency {
                    alias: "aws-smithy-json".into(),
                    package: "aws-smithy-json".into(),
                    requirement: "^0.63.0".into(),
                    kind: PublishedDependencyKind::Normal,
                    target: None,
                    optional: false,
                },
                PublishedDependency {
                    alias: "aws-smithy-http-server-065".into(),
                    package: "aws-smithy-http-server".into(),
                    requirement: "^0.65".into(),
                    kind: PublishedDependencyKind::Normal,
                    target: None,
                    optional: false,
                },
                PublishedDependency {
                    alias: "aws-smithy-async".into(),
                    package: "aws-smithy-async".into(),
                    requirement: "^1.2.0".into(),
                    kind: PublishedDependencyKind::Dev,
                    target: None,
                    optional: false,
                },
                PublishedDependency {
                    alias: "build-dep".into(),
                    package: "build-dep".into(),
                    requirement: "^1.0.0".into(),
                    kind: PublishedDependencyKind::Build,
                    target: None,
                    optional: true,
                },
                PublishedDependency {
                    alias: "windows-only".into(),
                    package: "windows-only".into(),
                    requirement: "^0.5.0".into(),
                    kind: PublishedDependencyKind::Normal,
                    target: Some("cfg(windows)".into()),
                    optional: false,
                },
                PublishedDependency {
                    alias: "explicitly-not-targeted".into(),
                    package: "explicitly-not-targeted".into(),
                    requirement: "^0.5.0".into(),
                    kind: PublishedDependencyKind::Normal,
                    target: None,
                    optional: false,
                },
            ],
            published.dependencies
        );
    }

    #[test]
    fn test_fake_index_file_rejects_unknown_version_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.toml");
        std::fs::write(
            &path,
            r#"
            [crates]
            aws-config = ["1.12.0"]

            [crate_version_metadata."aws-config@9.9.9"]
            dependencies = []
            "#,
        )
        .unwrap();
        let err = super::FakeIndex::try_from_file(&path).expect_err("version isn't published");
        assert!(
            format!("{err:#}").contains("does not refer to a version"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn test_fake_index_file_rejects_bad_kind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.toml");
        std::fs::write(
            &path,
            r#"
            [crates]
            aws-config = ["1.12.0"]

            [crate_version_metadata."aws-config@1.12.0"]
            dependencies = [
              { alias = "aws-smithy-json", requirement = "^0.63.0", kind = "runtime" },
            ]
            "#,
        )
        .unwrap();
        let err = super::FakeIndex::try_from_file(&path).expect_err("'runtime' isn't a kind");
        assert!(
            format!("{err:#}").contains("unrecognized dependency kind 'runtime'"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn test_fake_index_file_rejects_malformed_metadata_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.toml");
        std::fs::write(
            &path,
            r#"
            [crates]
            aws-config = ["1.12.0"]

            [crate_version_metadata."aws-config"]
            dependencies = []
            "#,
        )
        .unwrap();
        let err = super::FakeIndex::try_from_file(&path).expect_err("key has no version");
        assert!(
            format!("{err:#}").contains("must have the form 'name@version'"),
            "unexpected error: {err:#}"
        );
    }
}
