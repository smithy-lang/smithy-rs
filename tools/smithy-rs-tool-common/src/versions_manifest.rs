/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! This module provides structs with ser/de for working with the `versions.toml` file
//! in the root of the `aws-sdk-rust` repository.

use crate::package::PackageCategory;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::str::FromStr;

/// Root struct representing a `versions.toml` manifest
#[derive(Debug, Deserialize, Serialize)]
pub struct VersionsManifest {
    /// Git commit hash of the version of smithy-rs used to generate this SDK
    pub smithy_rs_revision: String,

    /// Git commit hash of the `aws-doc-sdk-examples` repository that was synced into this SDK
    pub aws_doc_sdk_examples_revision: String,

    /// All SDK crate version metadata
    pub crates: BTreeMap<String, CrateVersion>,

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

/// Release metadata
#[derive(Debug, Deserialize, Serialize)]
pub struct Release {
    /// The release tag associated with this `versions.toml`
    pub tag: String,

    /// Which crate versions were published with this release
    pub crates: BTreeMap<String, String>,
}

/// Version metadata for a crate
#[derive(Debug, Deserialize, Serialize)]
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
