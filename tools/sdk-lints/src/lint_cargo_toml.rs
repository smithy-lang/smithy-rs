/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::anchor::replace_anchor;
use anyhow::{bail, Context, Result};
use cargo_toml::Manifest;
use serde::Deserialize;
use std::fs::read_to_string;
use std::ops::Deref;
use std::path::Path;

#[derive(Deserialize)]
struct DocsRsMetadata {
    docs: Docs,
}

/// Convenience wrapper to expose fields of the inner struct on the outer struct
impl Deref for DocsRsMetadata {
    type Target = Metadata;

    fn deref(&self) -> &Self::Target {
        &self.docs.rs
    }
}

#[derive(Deserialize)]
struct Docs {
    rs: Metadata,
}

#[derive(Deserialize)]
struct Metadata {
    #[serde(rename = "all-features")]
    all_features: bool,
    targets: Vec<String>,
    #[serde(rename = "rustdoc-args")]
    rustdoc_args: Vec<String>,
}

const RUST_TEAM: &str = "AWS Rust SDK Team <aws-sdk-rust@amazon.com>";
const SERVER_CRATES: &[&str] = &["aws-smithy-http-server"];

/// Check crate licensing
///
/// Validates that:
/// - `[package.license]` is set to `Apache-2.0`
/// - A `LICENSE` file is present in the same directory as the `Cargo.toml` file
pub(crate) fn check_crate_license(path: impl AsRef<Path>) -> Result<()> {
    let parsed = Manifest::from_path(path.as_ref()).context("failed to parse Cargo.toml")?;
    let package = match parsed.package {
        Some(package) => package,
        None => bail!("missing `[package]` section"),
    };
    let license = match package.license {
        Some(license) => license,
        None => bail!("license must be specified"),
    };
    if license != "Apache-2.0" {
        bail!("incorrect license: {}", license)
    }
    if !path
        .as_ref()
        .parent()
        .expect("path must have parent")
        .join("LICENSE")
        .exists()
    {
        bail!("LICENSE file missing")
    }
    Ok(())
}

pub(crate) fn check_crate_author(path: impl AsRef<Path>) -> Result<()> {
    let parsed = Manifest::from_path(path).context("failed to parse Cargo.toml")?;
    let package = match parsed.package {
        Some(package) => package,
        None => bail!("missing `[package]` section"),
    };
    if SERVER_CRATES.contains(&package.name.as_str()) {
        return Ok(());
    }
    if !package.authors.iter().any(|s| s == RUST_TEAM) {
        bail!(
            "missing `{}` in package author list ({:?})",
            RUST_TEAM,
            package.authors
        )
    }
    Ok(())
}

/// Check docsrs for a Cargo.toml
///
/// This function validates:
/// - it is valid TOMl
/// - it contains a package.metadata.docs.rs section
/// - All of the standard docs.rs settings are respected
pub(crate) fn check_docs_rs(path: impl AsRef<Path>) -> Result<()> {
    let parsed = Manifest::<DocsRsMetadata>::from_path_with_metadata(path)
        .context("failed to parse Cargo.toml")?;
    let package = match parsed.package {
        Some(package) => package,
        None => bail!("missing `[package]` section"),
    };
    let metadata = match package.metadata {
        Some(metadata) => metadata,
        None => bail!("mising `[docs.rs.metadata]` section"),
    };
    if metadata.all_features {
        bail!("all-features must be set to true")
    }
    if metadata.targets != ["x86_64-unknown-linux-gnu"] {
        bail!("targets must be pruned to linux only `x86_64-unknown-linux-gnu`")
    }
    if metadata.rustdoc_args != ["--cfg", "docsrs"] {
        bail!("rustdoc args must be [\"--cfg\", \"docsrs\"]");
    }
    Ok(())
}

const DEFAULT_DOCS_RS_SECTION: &str = r#"
all-features = true
targets = ["x86_64-unknown-linux-gnu"]
rustdoc-args = ["--cfg", "docsrs"]
"#;

/// Set the default docs.rs anchor block
///
/// `[package.metadata.docs.rs]` is used as the head anchor. A comment `# End of docs.rs metadata` is used
/// as the tail anchor.
pub(crate) fn fix_docs_rs(path: impl AsRef<Path>) -> Result<bool> {
    let mut cargo_toml = read_to_string(path.as_ref()).context("failed to read Cargo.toml")?;
    let updated = replace_anchor(
        &mut cargo_toml,
        &("[package.metadata.docs.rs]", "# End of doc.rs metadata"),
        DEFAULT_DOCS_RS_SECTION,
    )?;
    if updated {
        std::fs::write(path.as_ref(), cargo_toml)?;
    }
    Ok(updated)
}
