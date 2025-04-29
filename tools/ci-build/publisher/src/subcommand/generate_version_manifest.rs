/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::fs::Fs;
use crate::package::discover_packages;
use anyhow::{bail, Context, Result};
use clap::Parser;
use semver::Version;
use serde::Deserialize;
use smithy_rs_tool_common::ci::is_preview_build;
use smithy_rs_tool_common::git::{find_git_repository_root, Git, GitCLI};
use smithy_rs_tool_common::package::PackageCategory;
use smithy_rs_tool_common::shell;
use smithy_rs_tool_common::versions_manifest::{CrateVersion, Release, VersionsManifest};
use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use tracing::info;

#[derive(Parser, Debug)]
pub struct GenerateVersionManifestArgs {
    /// Path to `smithy-build.json`
    #[clap(long)]
    smithy_build: PathBuf,
    /// Revision of `aws-doc-sdk-examples` repository used to retrieve examples
    #[clap(long)]
    examples_revision: String,
    /// Same as `input_location` but kept for backwards compatibility
    #[clap(long, required_unless_present = "input-location")]
    location: Option<PathBuf>,
    /// Path containing the generated SDK to generate a version manifest for
    #[clap(long, required_unless_present = "location")]
    input_location: Option<PathBuf>,
    /// Path to a directory in which a version manifest is generated
    #[clap(long, required_unless_present = "location")]
    output_location: Option<PathBuf>,
    /// Optional path to the `versions.toml` manifest from the previous SDK release
    #[clap(long)]
    previous_release_versions: Option<PathBuf>,
}

pub async fn subcommand_generate_version_manifest(
    GenerateVersionManifestArgs {
        smithy_build,
        examples_revision,
        location,
        input_location,
        output_location,
        previous_release_versions,
        ..
    }: &GenerateVersionManifestArgs,
) -> Result<()> {
    verify_crate_hasher_available()?;

    let repo_root = find_git_repository_root("smithy-rs", std::env::current_dir()?)?;
    let smithy_rs_revision = GitCLI::new(&repo_root)?
        .get_head_revision()
        .context("get smithy-rs revision")?;
    info!("Resolved smithy-rs revision to {}", smithy_rs_revision);

    let smithy_build_root = SmithyBuildRoot::from_file(smithy_build)?;
    let input_location = match (location, input_location) {
        (Some(location), None) => location,
        (None, Some(input_location)) => input_location,
        _ => bail!("Only one of `--location` or `--input-location` should be provided"),
    };
    let output_location = match (location, output_location) {
        (Some(location), None) => location,
        (None, Some(output_location)) => output_location,
        _ => bail!("Only one of `--location` or `--output-location` should be provided"),
    };
    let packages = discover_packages(Fs::Real, input_location)
        .await
        .context("read packages")?;

    let mut crates = BTreeMap::new();
    for package in packages {
        // Don't include examples
        if let PackageCategory::Unknown = package.category {
            continue;
        }

        let mut model_hash = None;
        if let PackageCategory::AwsSdk = package.category {
            if let Some(projection) = smithy_build_root
                .projections
                .get(&package.handle.name["aws-sdk-".len()..])
            {
                model_hash = Some(hash_models(projection)?);
            }
        }
        assert!(
            matches!(package.category, PackageCategory::AwsSdk) == model_hash.is_some(),
            "all generated SDK crates should have a model hash"
        );
        let version = package.handle.expect_version().to_string();
        crates.insert(
            package.handle.name,
            CrateVersion {
                category: package.category,
                version,
                source_hash: hash_crate(&package.crate_path).context("hash crate")?,
                model_hash,
            },
        );
    }
    info!("Discovered and hashed {} crates", crates.len());
    let mut versions_manifest = VersionsManifest {
        smithy_rs_revision: smithy_rs_revision.to_string(),
        aws_doc_sdk_examples_revision: examples_revision.to_string(),
        manual_interventions: Default::default(),
        crates,
        release: None,
    };

    versions_manifest.release =
        generate_release_metadata(&versions_manifest, previous_release_versions)?;

    let manifest_file_name = output_location.join("versions.toml");
    info!("Writing {:?}...", manifest_file_name);
    versions_manifest.write_to_file(&manifest_file_name)?;
    Ok(())
}

fn generate_release_metadata(
    versions_manifest: &VersionsManifest,
    maybe_previous_release_versions: &Option<PathBuf>,
) -> Result<Option<Release>> {
    // For release builds we generate a real release section
    if let (Some(previous_release_versions), false) =
        (maybe_previous_release_versions, is_preview_build())
    {
        let old_versions = VersionsManifest::from_file(previous_release_versions)?;
        Ok(Some(Release {
            tag: None,
            crates: find_released_versions(&old_versions, versions_manifest)?,
        }))
    // For preview builds we insert an empty section
    } else {
        Ok(Some(Release {
            tag: None,
            crates: BTreeMap::new(),
        }))
    }
}

fn parse_version(name: &str, value: &str) -> Result<Version> {
    match Version::parse(value) {
        Ok(version) => Ok(version),
        Err(err) => bail!(
            "Failed to parse version number `{}` from `{}`: {}",
            value,
            name,
            err
        ),
    }
}

fn find_released_versions(
    unrecent_versions: &VersionsManifest,
    recent_versions: &VersionsManifest,
) -> Result<BTreeMap<String, String>> {
    let mut released_versions = BTreeMap::new();
    for (crate_name, recent_version) in &recent_versions.crates {
        let recent_version = parse_version(crate_name, &recent_version.version)?;
        if let Some(unrecent_version) = unrecent_versions.crates.get(crate_name) {
            let unrecent_version = parse_version(crate_name, &unrecent_version.version)?;
            if unrecent_version != recent_version {
                // If the crate is in both version manifests with differing version
                // numbers, then it is part of the release
                released_versions.insert(crate_name.clone(), recent_version.to_string());
            }
        } else {
            // If the previous version manifest didn't have this crate, then it is part of this release
            released_versions.insert(crate_name.clone(), recent_version.to_string());
        }
    }
    // Sanity check: If a crate was previously included, but no longer is, we probably want to know about it
    let crates_to_remove = &unrecent_versions.manual_interventions.crates_to_remove;
    for unrecent_crate_name in unrecent_versions.crates.keys() {
        if !recent_versions.crates.contains_key(unrecent_crate_name)
            && !crates_to_remove.contains(unrecent_crate_name)
        {
            bail!(
                "Crate `{}` was included in the previous release's `versions.toml`, \
                 but is not included in the upcoming release. If this is expected, update the \
                 publisher tool to expect and allow it.",
                unrecent_crate_name
            )
        }
    }

    Ok(released_versions)
}

fn verify_crate_hasher_available() -> Result<()> {
    match std::process::Command::new("crate-hasher")
        .arg("--version")
        .spawn()
    {
        Ok(_) => Ok(()),
        Err(_) => {
            bail!(
                "This subcommand requires the `crate-hasher` tool to be on the PATH\n\
                Install it by going into tools/crate-hasher and running `cargo install --path .`."
            );
        }
    }
}

fn hash_crate(path: &Path) -> Result<String> {
    let output = std::process::Command::new("crate-hasher")
        .arg(path.to_str().expect("nothing special about these paths"))
        .output()?;
    shell::handle_failure("run crate-hasher", &output)?;
    let (stdout, _stderr) = shell::output_text(&output);
    Ok(stdout.trim().into())
}

fn hash_models(projection: &SmithyBuildProjection) -> Result<String> {
    // Must match `hashModels` in `CrateVersioner.kt`
    let mut hashes = String::new();
    for import in &projection.imports {
        hashes.push_str(&sha256::try_digest(import.as_path())?);
        hashes.push('\n');
    }
    Ok(sha256::digest(hashes))
}

#[derive(Debug, Deserialize)]
struct SmithyBuildRoot {
    projections: BTreeMap<String, SmithyBuildProjection>,
}

impl SmithyBuildRoot {
    fn from_file(path: &Path) -> Result<SmithyBuildRoot> {
        serde_json::from_reader(File::open(path).context("open smithy-build.json")?)
            .context("deserialize smithy-build.json")
    }
}

#[derive(Debug, Deserialize)]
struct SmithyBuildProjection {
    imports: Vec<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs_err as fs;
    use smithy_rs_tool_common::package::PackageCategory;
    use smithy_rs_tool_common::versions_manifest::ManualInterventions;
    use tempfile::TempDir;

    fn fake_manifest(
        crates: &[(&str, &str)],
        manual_interventions: Option<ManualInterventions>,
    ) -> VersionsManifest {
        VersionsManifest {
            smithy_rs_revision: "dontcare".into(),
            aws_doc_sdk_examples_revision: "dontcare".into(),
            manual_interventions: manual_interventions.unwrap_or_default(),
            crates: crates
                .iter()
                .map(|(name, version)| (name.to_string(), fake_version(version)))
                .collect(),
            release: None,
        }
    }
    fn fake_version(version: &str) -> CrateVersion {
        CrateVersion {
            category: PackageCategory::AwsSdk,
            version: version.into(),
            source_hash: "dontcare".into(),
            model_hash: None,
        }
    }

    #[test]
    fn test_find_released_versions_dropped_crate_sanity_check() {
        let result = find_released_versions(
            &fake_manifest(
                &[
                    ("aws-config", "0.11.0"),
                    ("aws-sdk-s3", "0.13.0"),
                    ("aws-sdk-dynamodb", "0.12.0"),
                ],
                None,
            ),
            &fake_manifest(
                &[
                    // oops, we lost aws-config
                    ("aws-sdk-s3", "0.13.0"),
                    ("aws-sdk-dynamodb", "0.12.0"),
                ],
                None,
            ),
        );
        assert!(result.is_err());
        let error = format!("{}", result.err().unwrap());
        assert!(
            error.starts_with("Crate `aws-config` was included in"),
            "Unexpected error: {}",
            error
        );
    }

    #[test]
    fn test_find_released_versions_dropped_crate_sanity_check_manual_intervention() {
        let result = find_released_versions(
            &fake_manifest(
                &[
                    ("aws-config", "0.11.0"),
                    ("aws-sdk-redshiftserverless", "0.13.0"),
                    ("aws-sdk-s3", "0.13.0"),
                    ("aws-sdk-dynamodb", "0.12.0"),
                ],
                Some(ManualInterventions {
                    crates_to_remove: vec!["aws-sdk-redshiftserverless".to_string()],
                }),
            ),
            &fake_manifest(
                &[
                    ("aws-config", "0.11.0"),
                    // we intentionally dropped aws-sdk-redshiftserverless
                    ("aws-sdk-s3", "0.13.0"),
                    ("aws-sdk-dynamodb", "0.12.0"),
                ],
                None,
            ),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_find_released_versions() {
        let result = find_released_versions(
            &fake_manifest(
                &[
                    ("aws-config", "0.11.0"),
                    ("aws-sdk-s3", "0.13.0"),
                    ("aws-sdk-dynamodb", "0.12.0"),
                ],
                None,
            ),
            &fake_manifest(
                &[
                    ("aws-config", "0.12.0"),          // updated
                    ("aws-sdk-s3", "0.14.0"),          // updated
                    ("aws-sdk-dynamodb", "0.12.0"),    // same
                    ("aws-sdk-somethingnew", "0.1.0"), // new
                ],
                None,
            ),
        )
        .unwrap();

        assert_eq!("0.12.0", result.get("aws-config").unwrap());
        assert_eq!("0.14.0", result.get("aws-sdk-s3").unwrap());
        assert_eq!("0.1.0", result.get("aws-sdk-somethingnew").unwrap());
        assert!(result.get("aws-sdk-dynamodb").is_none());
        assert_eq!(3, result.len());
    }

    #[test]
    fn test_hash_models() {
        let tmp = TempDir::new().unwrap();
        let model1a = tmp.path().join("model1a");
        let model1b = tmp.path().join("model1b");

        fs::write(&model1a, "foo").unwrap();
        fs::write(&model1b, "bar").unwrap();

        let hash = hash_models(&SmithyBuildProjection {
            imports: vec![model1a, model1b],
        })
        .unwrap();

        assert_eq!(
            "964021077fb6c3d42ae162ab2e2255be64c6d96a6d77bca089569774d54ef69b",
            hash
        );
    }
}
