/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Local filesystem git repository discovery. This enables the tool to
//! orient itself despite being run anywhere from within the git repo.

use crate::git;
use crate::shell::ShellOperation;
use crate::{SDK_REPO_CRATE_PATH, SDK_REPO_NAME};
use anyhow::Result;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Git repository containing crates to be published.
#[derive(Debug)]
pub struct Repository {
    pub root: PathBuf,
    pub current_tag: String,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to find {0} repository root")]
    RepositoryRootNotFound(String),
}

/// Given a `location`, this function looks for the `aws-sdk-rust` git repository. If found,
/// it resolves the `sdk/` directory. Otherwise, it returns the original `location`.
pub async fn resolve_publish_location(location: &Path) -> PathBuf {
    match find_git_repository_root(SDK_REPO_NAME, location).await {
        // If the given path was the `aws-sdk-rust` repo root, then resolve the `sdk/` directory to publish from
        Ok(sdk_repo) => sdk_repo.root.join(SDK_REPO_CRATE_PATH),
        // Otherwise, publish from the given path (likely the smithy-rs runtime bundle)
        Err(_) => location.into(),
    }
}

/// Attempts to find git repository root from the given location.
pub async fn find_git_repository_root(
    repo_name: &str,
    location: impl Into<PathBuf>,
) -> Result<Repository> {
    let mut current_dir = location.into();
    let os_name = OsStr::new(repo_name);
    loop {
        if is_git_root(&current_dir) {
            if let Some(file_name) = current_dir.file_name() {
                if os_name == file_name {
                    let current_tag = git::GetCurrentTag::new(&current_dir).spawn().await?;
                    return Ok(Repository {
                        root: current_dir,
                        current_tag,
                    });
                }
            }
            return Err(Error::RepositoryRootNotFound(repo_name.into()).into());
        } else if !current_dir.pop() {
            return Err(Error::RepositoryRootNotFound(repo_name.into()).into());
        }
    }
}

fn is_git_root(path: &Path) -> bool {
    let path = path.join(".git");
    path.exists() && path.is_dir()
}
