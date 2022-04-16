/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use anyhow::{bail, Result};
use smithy_rs_tool_common::shell::{handle_failure, output_text};
use std::borrow::Cow;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitHash(String);

impl<T: Into<String>> From<T> for CommitHash {
    fn from(hash: T) -> Self {
        CommitHash(hash.into())
    }
}

impl AsRef<str> for CommitHash {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CommitHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Debug)]
pub struct Commit {
    pub hash: CommitHash,
    pub author_name: String,
    pub author_email: String,
    pub message_subject: String,
    pub message_body: String,
}

impl Commit {
    pub fn message(&self) -> Cow<'_, str> {
        if self.message_body.is_empty() {
            Cow::Borrowed(&self.message_subject)
        } else {
            Cow::Owned(format!("{}\n\n{}", self.message_subject, self.message_body))
        }
    }
}

/// Easily mockable interface with Git for testing
#[cfg_attr(test, mockall::automock)]
pub trait Git {
    /// Returns the repository path
    fn path(&self) -> &Path;

    /// Returns commit hash of HEAD (i.e., `git rev-parse HEAD`)
    fn get_head_revision(&self) -> Result<CommitHash>;

    /// Stages the given path (i.e., `git add ${path}`)
    fn stage(&self, path: &Path) -> Result<()>;

    /// Commits the staged files on behalf of the given author using a bot commiter.
    fn commit_on_behalf(
        &self,
        bot_name: &str,
        bot_email: &str,
        author_name: &str,
        author_email: &str,
        message: &str,
    ) -> Result<()>;

    /// Commits staged files.
    fn commit(&self, name: &str, email: &str, message: &str) -> Result<()>;

    /// Returns a list of commit hashes in reverse chronological order starting with
    /// `start_inclusive_revision` and ending before `end_exclusive_revision`. Both of
    /// these arguments can be any kind of Git revision (e.g., HEAD, HEAD~2, commit hash, etc).
    fn rev_list<'a>(
        &self,
        start_inclusive_revision: &str,
        end_exclusive_revision: &str,
        path: Option<&'a Path>,
    ) -> Result<Vec<CommitHash>>;

    /// Returns information about a given revision.
    fn show(&self, revision: &str) -> Result<Commit>;

    /// Hard resets to the given revision.
    fn hard_reset(&self, revision: &str) -> Result<()>;
}

enum CommitInfo {
    CommitHash,
    AuthorName,
    AuthorEmail,
    MessageSubject,
    MessageBody,
}

pub struct GitCLI {
    repo_path: PathBuf,
    binary_name: String,
}

impl GitCLI {
    pub fn new(repo_path: &Path) -> Result<Self> {
        if !repo_path.join(".git").is_dir() {
            bail!("{:?} is not a git repository", repo_path);
        }
        Ok(Self {
            repo_path: repo_path.into(),
            binary_name: "git".into(),
        })
    }

    #[cfg(test)]
    pub fn with_binary(repo_path: &Path, name: &str) -> Self {
        Self {
            repo_path: repo_path.into(),
            binary_name: name.into(),
        }
    }

    fn extract_commit_info(&self, revision: &str, info: CommitInfo) -> Result<String> {
        let mut command = Command::new(&self.binary_name);
        command.arg("show");
        command.arg("-s");
        command.arg(revision);
        command.arg(format!(
            "--format={}",
            match info {
                CommitInfo::CommitHash => "%H",
                CommitInfo::AuthorName => "%an",
                CommitInfo::AuthorEmail => "%ae",
                CommitInfo::MessageSubject => "%s",
                CommitInfo::MessageBody => "%b",
            }
        ));
        command.current_dir(&self.repo_path);

        let output = command.output()?;
        handle_failure("extract_commit_info", &output)?;
        let (stdout, _) = output_text(&output);
        Ok(stdout.trim().into())
    }
}

impl Git for GitCLI {
    fn path(&self) -> &Path {
        &self.repo_path
    }

    fn get_head_revision(&self) -> Result<CommitHash> {
        let mut command = Command::new(&self.binary_name);
        command.arg("rev-parse");
        command.arg("HEAD");
        command.current_dir(&self.repo_path);

        let output = command.output()?;
        handle_failure("get_head_revision", &output)?;
        let (stdout, _) = output_text(&output);
        Ok(CommitHash(stdout.trim().into()))
    }

    fn stage(&self, path: &Path) -> Result<()> {
        let mut command = Command::new(&self.binary_name);
        command.arg("add");
        command.arg(path);
        command.current_dir(&self.repo_path);

        let output = command.output()?;
        handle_failure("stage", &output)?;
        Ok(())
    }

    fn commit_on_behalf(
        &self,
        bot_name: &str,
        bot_email: &str,
        author_name: &str,
        author_email: &str,
        message: &str,
    ) -> Result<()> {
        let mut command = Command::new(&self.binary_name);
        command.arg("-c");
        command.arg(format!("user.name={}", bot_name));
        command.arg("-c");
        command.arg(format!("user.email={}", bot_email));
        command.arg("commit");
        command.arg("-m");
        command.arg(message);
        command.arg("--author");
        command.arg(format!("{} <{}>", author_name, author_email));
        command.current_dir(&self.repo_path);

        let output = command.output()?;
        handle_failure("commit_on_behalf", &output)?;
        Ok(())
    }

    fn commit(&self, name: &str, email: &str, message: &str) -> Result<()> {
        let mut command = Command::new(&self.binary_name);
        command.arg("-c");
        command.arg(format!("user.name={}", name));
        command.arg("-c");
        command.arg(format!("user.email={}", email));
        command.arg("commit");
        command.arg("-m");
        command.arg(message);
        command.current_dir(&self.repo_path);

        let output = command.output()?;
        handle_failure("commit", &output)?;
        Ok(())
    }

    fn rev_list(
        &self,
        start_inclusive_revision: &str,
        end_exclusive_revision: &str,
        path: Option<&Path>,
    ) -> Result<Vec<CommitHash>> {
        let mut command = Command::new(&self.binary_name);
        command.arg("rev-list");
        command.arg(format!(
            "{}..{}",
            end_exclusive_revision, start_inclusive_revision
        ));
        if let Some(path) = path {
            command.arg("--");
            command.arg(path);
        }
        command.current_dir(&self.repo_path);

        let output = command.output()?;
        handle_failure("rev_list", &output)?;
        let (stdout, _) = output_text(&output);
        Ok(stdout
            .split_ascii_whitespace()
            .into_iter()
            .map(CommitHash::from)
            .collect())
    }

    fn show(&self, revision: &str) -> Result<Commit> {
        Ok(Commit {
            hash: CommitHash::from(self.extract_commit_info(revision, CommitInfo::CommitHash)?),
            author_name: self.extract_commit_info(revision, CommitInfo::AuthorName)?,
            author_email: self.extract_commit_info(revision, CommitInfo::AuthorEmail)?,
            message_subject: self.extract_commit_info(revision, CommitInfo::MessageSubject)?,
            message_body: self.extract_commit_info(revision, CommitInfo::MessageBody)?,
        })
    }

    fn hard_reset(&self, revision: &str) -> Result<()> {
        let mut command = Command::new(&self.binary_name);
        command.arg("reset");
        command.arg("--hard");
        command.arg(revision);
        command.current_dir(&self.repo_path);

        let output = command.output()?;
        handle_failure("rev_list", &output)?;
        Ok(())
    }
}
