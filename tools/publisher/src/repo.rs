/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Local filesystem git repository discovery. This enables the tool to
//! orient itself despite being run anywhere from within the git repo.

use crate::{SDK_REPO_CRATE_PATH, SDK_REPO_NAME};
use anyhow::Result;
use smithy_rs_tool_common::git;
use smithy_rs_tool_common::shell::ShellOperation;
use std::path::{Path, PathBuf};

/// Git repository containing crates to be published.
#[derive(Debug)]
pub struct Repository {
    pub root: PathBuf,
}

impl Repository {
    pub fn new(repo_name: &str, path: impl Into<PathBuf>) -> Result<Repository> {
        let root = git::find_git_repository_root(repo_name, path.into())?;
        Ok(Repository { root })
    }

    /// Returns the current tag of this repository
    pub async fn current_tag(&self) -> Result<String> {
        git::GetCurrentTag::new(&self.root).spawn().await
    }
}

/// Given a `location`, this function looks for the `aws-sdk-rust` git repository. If found,
/// it resolves the `sdk/` directory. Otherwise, it returns the original `location`.
pub fn resolve_publish_location(location: &Path) -> PathBuf {
    match Repository::new(SDK_REPO_NAME, location) {
        // If the given path was the `aws-sdk-rust` repo root, then resolve the `sdk/` directory to publish from
        Ok(sdk_repo) => sdk_repo.root.join(SDK_REPO_CRATE_PATH),
        // Otherwise, publish from the given path (likely the smithy-rs runtime bundle)
        Err(_) => location.into(),
    }
}
