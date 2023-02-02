/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Module for interacting with Cargo.

mod add_owner;
mod get_owners;
mod publish;
mod remove_owner;
mod yank;

pub use add_owner::AddOwner;
pub use get_owners::GetOwners;
pub use publish::Publish;
pub use remove_owner::RemoveOwner;
pub use yank::Yank;

use anyhow::{Context, Result};
use smithy_rs_tool_common::shell::handle_failure;
use std::process::Command;

/// Confirms that cargo exists on the path.
pub fn confirm_installed_on_path() -> Result<()> {
    handle_failure(
        "discover cargo version",
        &Command::new("cargo")
            .arg("version")
            .output()
            .context("cargo is not installed on the PATH")?,
    )
    .context("cargo is not installed on the PATH")
}
