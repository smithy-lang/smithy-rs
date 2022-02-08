/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::shell::ShellOperation;
use anyhow::{bail, Result};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

mod checkout_revision;
pub use checkout_revision::CheckoutRevision;

mod get_current_tag;
pub use get_current_tag::GetCurrentTag;

mod get_last_commit;
pub use get_last_commit::GetLastCommit;

mod get_repo_root;
pub use get_repo_root::GetRepoRoot;

/// Attempts to find git repository root from the given location.
pub fn find_git_repository_root(repo_name: &str, location: impl AsRef<Path>) -> Result<PathBuf> {
    let path = GetRepoRoot::new(location.as_ref()).run()?;
    if path.file_name() == Some(OsStr::new(repo_name)) {
        Ok(path)
    } else {
        bail!("failed to find {} repository root", repo_name);
    }
}
