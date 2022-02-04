/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use anyhow::{bail, Result};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

mod checkout_revision;
pub use checkout_revision::CheckoutRevision;

mod get_current_tag;
pub use get_current_tag::GetCurrentTag;

mod get_last_commit;
pub use get_last_commit::GetLastCommit;

/// Attempts to find git repository root from the given location.
pub fn find_git_repository_root(repo_name: &str, location: impl AsRef<Path>) -> Result<PathBuf> {
    let mut current_dir = location.as_ref().canonicalize()?;
    let os_name = OsStr::new(repo_name);
    loop {
        if is_git_root(&current_dir) {
            if let Some(file_name) = current_dir.file_name() {
                if os_name == file_name {
                    return Ok(current_dir);
                }
            }
            break;
        } else if !current_dir.pop() {
            break;
        }
    }
    bail!("failed to find {0} repository root", repo_name)
}

fn is_git_root(path: &Path) -> bool {
    let path = path.join(".git");
    path.exists() && path.is_dir()
}
