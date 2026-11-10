/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// Lints for the crates in the `dynamic-client/` workspace.
//
// CI builds the runtime crates in `check-rust-runtimes`, and that script also checks their MSRV.
// CI does not build the dynamic client crates, so these lints check them by reading files.

use std::fs::read_to_string;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::lint::LintError;
use crate::{dynamic_client_crates, repo_root, Check, Lint};

// Checks that each dynamic client crate sets `rust-version` to the MSRV in
// `rust-toolchain.toml`.
//
// The comparison ignores the patch version, as `check-rust-runtimes` does for runtime crates.
pub(crate) struct DynamicClientMsrv {
    msrv: String,
}

impl DynamicClientMsrv {
    pub(crate) fn new() -> Result<Self> {
        let path = repo_root().join("rust-toolchain.toml");
        let contents =
            read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
        let toolchain: toml::Value = toml::from_str(&contents)?;
        let channel = toolchain
            .get("toolchain")
            .and_then(|t| t.get("channel"))
            .and_then(|c| c.as_str())
            .with_context(|| format!("{} has no toolchain.channel", path.display()))?;
        Ok(Self {
            msrv: major_minor(channel).to_string(),
        })
    }
}

impl Lint for DynamicClientMsrv {
    fn name(&self) -> &str {
        "Dynamic client crates use the MSRV"
    }

    fn files_to_check(&self) -> Result<Vec<PathBuf>> {
        Ok(dynamic_client_crates()?
            .map(|crte| crte.join("Cargo.toml"))
            .collect())
    }
}

impl Check for DynamicClientMsrv {
    fn check(&self, path: impl AsRef<Path>) -> Result<Vec<LintError>> {
        let manifest: toml::Value = toml::from_str(&read_to_string(path)?)?;
        let rust_version = manifest
            .get("package")
            .and_then(|p| p.get("rust-version"))
            .and_then(|v| v.as_str());
        Ok(match rust_version {
            None => vec![LintError::new("crate must set `rust-version`")],
            Some(version) if major_minor(version) != self.msrv => vec![LintError::new(format!(
                "`rust-version` is {version}, but the MSRV in rust-toolchain.toml is {}",
                self.msrv
            ))],
            Some(_) => vec![],
        })
    }
}

// Checks that each dynamic client crate forbids `unsafe` code.
//
// Without `unsafe` code, the crates need no Miri or sanitizer runs.
pub(crate) struct DynamicClientForbidsUnsafe;

const FORBID_UNSAFE: &str = "#![forbid(unsafe_code)]";

impl Lint for DynamicClientForbidsUnsafe {
    fn name(&self) -> &str {
        "Dynamic client crates forbid unsafe code"
    }

    fn files_to_check(&self) -> Result<Vec<PathBuf>> {
        Ok(dynamic_client_crates()?
            .map(|crte| crte.join("src/lib.rs"))
            .collect())
    }
}

impl Check for DynamicClientForbidsUnsafe {
    fn check(&self, path: impl AsRef<Path>) -> Result<Vec<LintError>> {
        if read_to_string(path)?.contains(FORBID_UNSAFE) {
            Ok(vec![])
        } else {
            Ok(vec![LintError::new(format!(
                "lib.rs must contain `{FORBID_UNSAFE}`"
            ))])
        }
    }
}

// Returns `1.94` for `1.94.1`, and the input itself when it has no patch version.
fn major_minor(version: &str) -> &str {
    match version.match_indices('.').nth(1) {
        Some((index, _)) => &version[..index],
        None => version,
    }
}

#[cfg(test)]
mod tests {
    use super::major_minor;

    #[test]
    fn major_minor_drops_the_patch_version() {
        assert_eq!("1.94", major_minor("1.94.1"));
        assert_eq!("1.94", major_minor("1.94"));
        assert_eq!("stable", major_minor("stable"));
    }
}
