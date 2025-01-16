/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::lint::LintError;
use crate::{repo_root, Check, Lint};
use anyhow::Result;
use smithy_rs_tool_common::changelog::{ChangelogLoader, ValidationSet};
use std::fmt::Debug;
use std::path::{Path, PathBuf};

pub(crate) struct ChangelogNext;

impl Lint for ChangelogNext {
    fn name(&self) -> &str {
        "Changelog.next"
    }

    fn files_to_check(&self) -> Result<Vec<PathBuf>> {
        Ok(vec![repo_root().join("CHANGELOG.next.toml")])
    }
}

impl Check for ChangelogNext {
    fn check(&self, path: impl AsRef<Path> + Debug) -> Result<Vec<LintError>> {
        if path.as_ref().exists() {
            Ok(vec![LintError::new(
                "the legacy `CHANGELOG.next.toml` should no longer exist",
            )])
        } else {
            Ok(vec![])
        }
    }
}

#[allow(dead_code)]
pub(crate) struct DotChangelog;

impl Lint for DotChangelog {
    fn name(&self) -> &str {
        ".changelog"
    }

    fn files_to_check(&self) -> Result<Vec<PathBuf>> {
        Ok(vec![repo_root().join(".changelog")])
    }
}

impl Check for DotChangelog {
    fn check(&self, path: impl AsRef<Path> + Debug) -> Result<Vec<LintError>> {
        match check_changelog_next(path) {
            Ok(_) => Ok(vec![]),
            Err(errs) => Ok(errs),
        }
    }
}

/// Validate that changelog entries in the `.changelog` directory follows best practices
#[allow(dead_code)]
fn check_changelog_next(path: impl AsRef<Path> + Debug) -> std::result::Result<(), Vec<LintError>> {
    let parsed = ChangelogLoader::default()
        .load_from_dir(path)
        .map_err(|e| vec![LintError::via_display(e)])?;
    parsed
        .validate(ValidationSet::Development)
        .map_err(|errs| {
            errs.into_iter()
                .map(LintError::via_display)
                .collect::<Vec<_>>()
        })?;
    Ok(())
}
