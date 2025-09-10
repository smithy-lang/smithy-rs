/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! This module provides structs with ser/de for working with the `versions.toml` file
//! in the root of the `aws-sdk-rust` repository.

use crate::package::PackageCategory;
use crate::release_tag::ReleaseTag;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::str::FromStr;

pub type CrateVersionMetadataMap = BTreeMap<String, CrateVersion>;

/// Root struct representing a `versions.toml` manifest
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct VersionsManifest {
    /// Git commit hash of the version of smithy-rs used to generate this SDK
    pub smithy_rs_revision: String,

    // TODO(examples removal post cleanup): Remove this field once examples revision is removed
    // from `versions.toml` in the main branch of `aws-sdk-rust` repository.
    /// Git commit hash of the `aws-doc-sdk-examples` repository that was synced into this SDK
    pub aws_doc_sdk_examples_revision: Option<String>,

    /// Optional manual interventions to apply to the next release.
    /// These are intended to be filled out manually in the `versions.toml` via pull request
    /// to `aws-sdk-rust`.
    #[serde(default)]
    pub manual_interventions: ManualInterventions,

    /// All SDK crate version metadata
    pub crates: CrateVersionMetadataMap,

    /// Crate versions that were a part of this SDK release.
    /// Releases may not release every single crate, which can happen if a crate has no changes.
    ///
    /// This member is optional since a one-off smoke test SDK in local development
    /// isn't going to have a previous release to reflect upon to determine release metadata.
    pub release: Option<Release>,
}

impl VersionsManifest {
    pub fn from_file(path: impl AsRef<Path>) -> Result<VersionsManifest> {
        Self::from_str(
            &fs::read_to_string(path.as_ref())
                .with_context(|| format!("Failed to read {:?}", path.as_ref()))?,
        )
        .with_context(|| format!("Failed to parse {:?}", path.as_ref()))
    }

    pub async fn from_github_tag(tag: &ReleaseTag) -> Result<VersionsManifest> {
        let manifest_url = format!(
            "https://raw.githubusercontent.com/awslabs/aws-sdk-rust/{}/versions.toml",
            tag
        );
        let manifest_contents = reqwest::get(manifest_url)
            .await
            .context("failed to download release manifest")?
            .text()
            .await
            .context("failed to download release manifest content")?;
        Self::from_str(&manifest_contents).context("failed to parse versions.toml file")
    }

    pub fn write_to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let serialized = toml::to_string_pretty(self)
            .context("failed to serialize versions manifest into TOML")?;
        fs::write(path.as_ref(), serialized)
            .with_context(|| format!("failed to write to {:?}", path.as_ref()))?;
        Ok(())
    }
}

impl FromStr for VersionsManifest {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(toml::from_str(value)?)
    }
}

/// The SDK release process has sanity checks sprinkled throughout it to make sure
/// a release is done correctly. Sometimes, manual intervention is required to bypass
/// these sanity checks. For example, when a service model is intentionally removed,
/// without manual intervention, there would be no way to release that removal.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct ManualInterventions {
    /// List of crate names that are being removed from the SDK in the next release.
    ///
    /// __Note:__ this only bypasses a release-time sanity check. The models for these crates
    /// (if they're generated) need to be manually deleted, and the crates must be manually
    /// yanked after the release (if necessary).
    #[serde(default)]
    pub crates_to_remove: Vec<String>,
}

/// Release metadata
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct Release {
    /// The release tag associated with this `versions.toml`
    pub tag: Option<String>,

    /// Which crate versions were published with this release
    pub crates: BTreeMap<String, String>,
}

/// Version metadata for a crate
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct CrateVersion {
    /// What kind of crate this is. Is it the Smithy runtime? AWS runtime? SDK crate?
    pub category: PackageCategory,

    /// Version of the crate.
    pub version: String,

    /// The hash of the crate source code as determined by the `crate-hasher` tool in smithy-rs.
    pub source_hash: String,

    /// The SHA-256 hash of the AWS model file(s) used to generate this crate (if this is a SDK crate).
    pub model_hash: Option<String>,
}
