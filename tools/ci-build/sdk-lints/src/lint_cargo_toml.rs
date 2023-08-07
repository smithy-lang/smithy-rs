/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::anchor::replace_anchor;
use crate::lint::LintError;
use crate::{all_cargo_tomls, Check, Fix, Lint};
use anyhow::{Context, Result};
use cargo_toml::{Manifest, Package};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::fs::read_to_string;
use std::ops::Deref;
use std::path::{Path, PathBuf};

macro_rules! return_errors {
    ($errs: expr) => {
        match $errs {
            Ok(res) => res,
            Err(errs) => return Ok(errs),
        }
    };
}

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

const RUST_SDK_TEAM: &str = "AWS Rust SDK Team <aws-sdk-rust@amazon.com>";
const SERVER_TEAM: &str = "Smithy Rust Server <smithy-rs-server@amazon.com>";
const SERVER_CRATES: &[&str] = &[
    "aws-smithy-http-server",
    "aws-smithy-http-server-python",
    "aws-smithy-http-server-typescript",
];

/// Check crate licensing
///
/// Validates that:
/// - `[package.license]` is set to `Apache-2.0`
/// - A `LICENSE` file is present in the same directory as the `Cargo.toml` file
pub(crate) struct CrateLicense;

impl Lint for CrateLicense {
    fn name(&self) -> &str {
        "Crate licensing"
    }

    fn files_to_check(&self) -> Result<Vec<PathBuf>> {
        Ok(all_cargo_tomls()?.collect())
    }
}

impl Check for CrateLicense {
    fn check(&self, path: impl AsRef<Path>) -> Result<Vec<LintError>> {
        let package = return_errors!(package(&path)?);
        check_crate_license(package, path)
    }
}

fn check_crate_license(package: Package, path: impl AsRef<Path>) -> Result<Vec<LintError>> {
    let mut errors = vec![];
    match package.license {
        Some(license) if license == "Apache-2.0" => {}
        incorrect_license => errors.push(LintError::new(format!(
            "invalid license: {:?}",
            incorrect_license
        ))),
    };
    if !path
        .as_ref()
        .parent()
        .expect("path must have parent")
        .join("LICENSE")
        .exists()
    {
        errors.push(LintError::new("LICENSE file missing"));
    }
    Ok(errors)
}

/// Checks that the `author` field of the crate is either the Rust SDK team or the server SDK Team
pub(crate) struct CrateAuthor;

impl Lint for CrateAuthor {
    fn name(&self) -> &str {
        "Crate author"
    }

    fn files_to_check(&self) -> Result<Vec<PathBuf>> {
        Ok(all_cargo_tomls()?.collect())
    }
}

impl Check for CrateAuthor {
    fn check(&self, path: impl AsRef<Path>) -> Result<Vec<LintError>> {
        let package = return_errors!(package(path)?);
        check_crate_author(package)
    }
}

fn package<T>(path: impl AsRef<Path>) -> Result<std::result::Result<Package<T>, Vec<LintError>>>
where
    T: DeserializeOwned,
{
    let parsed = Manifest::from_path_with_metadata(path).context("failed to parse Cargo.toml")?;
    match parsed.package {
        Some(package) => Ok(Ok(package)),
        None => Ok(Err(vec![LintError::new("missing `[package]` section")])),
    }
}

fn check_crate_author(package: Package) -> Result<Vec<LintError>> {
    let mut errors = vec![];
    let expected_author = if SERVER_CRATES.contains(&package.name.as_str()) {
        SERVER_TEAM
    } else {
        RUST_SDK_TEAM
    };
    if !package.authors.iter().any(|s| s == expected_author) {
        errors.push(LintError::new(format!(
            "missing `{}` in package author list ({:?})",
            expected_author, package.authors
        )));
    }
    Ok(errors)
}

pub(crate) struct DocsRs;

impl Lint for DocsRs {
    fn name(&self) -> &str {
        "Cargo.toml package.metadata.docs.rs"
    }

    fn files_to_check(&self) -> Result<Vec<PathBuf>> {
        Ok(all_cargo_tomls()?.collect())
    }
}

impl Fix for DocsRs {
    fn fix(&self, path: impl AsRef<Path>) -> Result<(Vec<LintError>, String)> {
        let contents = read_to_string(&path)?;
        let updated = fix_docs_rs(&contents)?;
        let package = match package(path) {
            Ok(Ok(package)) => package,
            Ok(Err(errs)) => return Ok((errs, updated)),
            Err(errs) => return Ok((vec![LintError::new(format!("{}", errs))], updated)),
        };
        let lint_errors = check_docs_rs(&package);
        Ok((lint_errors, updated))
    }
}

/// Check Cargo.toml for a valid docs.rs metadata section
///
/// This function validates:
/// - it is valid TOML
/// - it contains a package.metadata.docs.rs section
/// - All of the standard docs.rs settings are respected
fn check_docs_rs(package: &Package<DocsRsMetadata>) -> Vec<LintError> {
    let metadata = match &package.metadata {
        Some(metadata) => metadata,
        None => return vec![LintError::new("missing docs.rs metadata section")],
    };
    let mut errs = vec![];
    if !metadata.all_features {
        errs.push(LintError::new("all-features must be set to true"))
    }
    if metadata.targets != ["x86_64-unknown-linux-gnu"] {
        errs.push(LintError::new(
            "targets must be pruned to linux only `x86_64-unknown-linux-gnu`",
        ))
    }
    if metadata.rustdoc_args != ["--cfg", "docsrs"] {
        errs.push(LintError::new(
            "rustdoc args must be [\"--cfg\", \"docsrs\"]",
        ));
    }
    errs
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
fn fix_docs_rs(contents: &str) -> Result<String> {
    let mut new = contents.to_string();
    replace_anchor(
        &mut new,
        &("[package.metadata.docs.rs]", "# End of docs.rs metadata"),
        DEFAULT_DOCS_RS_SECTION,
    )?;
    Ok(new)
}
