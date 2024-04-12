/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::util::utf8_path_buf;
use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use smithy_rs_tool_common::git::find_git_repository_root;
use std::{env, ffi::OsStr, process::Command};

/// Git repository
pub struct Repo {
    pub root: Utf8PathBuf,
}

impl Repo {
    pub fn new(maybe_root: Option<&Utf8Path>) -> Result<Self> {
        Ok(Self {
            root: repo_root(maybe_root)?,
        })
    }

    /// Returns a `std::process::Command` set to run git in this repo with the given args
    pub fn git<I, S>(&self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.cmd("git", args)
    }

    /// Returns a `std::process::Command` set to run a shell command in this repo with the given args
    pub fn cmd<I, S>(&self, cmd: &str, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = Command::new(cmd);
        cmd.current_dir(&self.root);
        cmd.args(args);
        cmd
    }
}

fn repo_root(hint: Option<&Utf8Path>) -> Result<Utf8PathBuf> {
    let cwd = utf8_path_buf(env::current_dir().context("failed to get current working directory")?);
    Ok(utf8_path_buf(find_git_repository_root(
        "smithy-rs",
        hint.unwrap_or(&cwd),
    )?))
}
