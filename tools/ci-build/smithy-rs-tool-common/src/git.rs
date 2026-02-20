/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::release_tag::ReleaseTag;
use crate::shell::{handle_failure, output_text};
use anyhow::{bail, Context, Result};
use std::borrow::Cow;
use std::ffi::OsStr;
use std::fmt::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;
use tracing::debug;
use tracing::warn;

/// Attempts to find git repository root from the given location.
pub fn find_git_repository_root(repo_name: &str, location: impl AsRef<Path>) -> Result<PathBuf> {
    let output = Command::new("git")
        .arg("rev-parse")
        .arg("--show-toplevel")
        .current_dir(location.as_ref())
        .output()
        .context("failed to run git")?;
    handle_failure("determine git repo root", &output)?;

    let (stdout, _) = output_text(&output);
    let path = PathBuf::from(stdout.trim());
    if path.file_name() != Some(OsStr::new(repo_name)) {
        warn!(
            "repository root {:?} doesn't have expected name '{}'",
            path, repo_name
        );
    }
    Ok(path)
}

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

#[derive(Clone, Debug, Eq, PartialEq)]
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
pub trait Git: Send + Sync {
    /// Returns the repository path
    fn path(&self) -> &Path;

    /// Clones the repository to the given path
    fn clone_to(&self, path: &Path) -> Result<()>;

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
    fn rev_list(
        &self,
        start_inclusive_revision: &str,
        end_exclusive_revision: &str,
        path: Option<&Path>,
    ) -> Result<Vec<CommitHash>>;

    /// Returns information about a given revision.
    fn show(&self, revision: &str) -> Result<Commit>;

    /// Hard resets to the given revision.
    fn hard_reset(&self, revision: &str) -> Result<()>;

    /// Returns the name of the current branch.
    fn current_branch_name(&self) -> Result<String>;

    /// Creates a branch at the given revision.
    fn create_branch(&self, branch_name: &str, revision: &str) -> Result<()>;

    /// Deletes a branch.
    fn delete_branch(&self, branch_name: &str) -> Result<()>;

    /// Squash merges a branch into the current branch and leaves the changes staged.
    fn squash_merge(&self, author_name: &str, author_email: &str, branch_name: &str) -> Result<()>;

    /// Returns list of untracked files.
    fn untracked_files(&self) -> Result<Vec<PathBuf>>;

    /// Returns list of changed files.
    fn changed_files(&self) -> Result<Vec<PathBuf>>;

    /// Finds the most recent tag that is reachable from `HEAD`.
    fn get_current_tag(&self) -> Result<ReleaseTag>;
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
        if !repo_path.join(".git").exists() {
            bail!("{repo_path:?} is not a git repository");
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

        let output = log_command(command).output()?;
        handle_failure("extract_commit_info", &output)?;
        let (stdout, _) = output_text(&output);
        Ok(stdout.trim().into())
    }
}

impl Git for GitCLI {
    fn path(&self) -> &Path {
        &self.repo_path
    }

    fn clone_to(&self, path: &Path) -> Result<()> {
        let mut command = Command::new(&self.binary_name);
        command.arg("clone");
        command.arg(&self.repo_path);
        command.current_dir(path);

        let output = log_command(command).output()?;
        handle_failure("clone_to", &output)?;
        Ok(())
    }

    fn get_head_revision(&self) -> Result<CommitHash> {
        let mut command = Command::new(&self.binary_name);
        command.arg("rev-parse");
        command.arg("HEAD");
        command.current_dir(&self.repo_path);

        let output = log_command(command).output()?;
        handle_failure("get_head_revision", &output)?;
        let (stdout, _) = output_text(&output);
        Ok(CommitHash(stdout.trim().into()))
    }

    fn stage(&self, path: &Path) -> Result<()> {
        let mut command = Command::new(&self.binary_name);
        command.arg("add");
        command.arg(path);
        command.current_dir(&self.repo_path);

        let output = log_command(command).output()?;
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
        command.arg(format!("user.name={bot_name}"));
        command.arg("-c");
        command.arg(format!("user.email={bot_email}"));
        command.arg("commit");
        command.arg("-m");
        command.arg(message);
        command.arg("--author");
        command.arg(format!("{author_name} <{author_email}>"));
        command.current_dir(&self.repo_path);

        let output = log_command(command).output()?;
        handle_failure("commit_on_behalf", &output)?;
        Ok(())
    }

    fn commit(&self, name: &str, email: &str, message: &str) -> Result<()> {
        let mut command = Command::new(&self.binary_name);
        command.arg("-c");
        command.arg(format!("user.name={name}"));
        command.arg("-c");
        command.arg(format!("user.email={email}"));
        command.arg("commit");
        command.arg("-m");
        command.arg(message);
        command.current_dir(&self.repo_path);

        let output = log_command(command).output()?;
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
            "{end_exclusive_revision}..{start_inclusive_revision}"
        ));
        if let Some(path) = path {
            command.arg("--");
            command.arg(path);
        }
        command.current_dir(&self.repo_path);

        let output = log_command(command).output()?;
        handle_failure("rev_list", &output)?;
        let (stdout, _) = output_text(&output);
        Ok(stdout
            .split_ascii_whitespace()
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

        let output = log_command(command).output()?;
        handle_failure("hard_reset", &output)?;
        Ok(())
    }

    fn current_branch_name(&self) -> Result<String> {
        let mut command = Command::new(&self.binary_name);
        command.arg("rev-parse");
        command.arg("--abbrev-ref");
        command.arg("HEAD");
        command.current_dir(&self.repo_path);

        let output = log_command(command).output()?;
        handle_failure("current_branch_name", &output)?;
        let (stdout, _) = output_text(&output);
        Ok(stdout.trim().into())
    }

    fn create_branch(&self, branch_name: &str, revision: &str) -> Result<()> {
        let mut command = Command::new(&self.binary_name);
        command.arg("branch");
        command.arg(branch_name);
        command.arg(revision);
        command.current_dir(&self.repo_path);

        let output = log_command(command).output()?;
        handle_failure("create_branch", &output)?;
        Ok(())
    }

    fn delete_branch(&self, branch_name: &str) -> Result<()> {
        let mut command = Command::new(&self.binary_name);
        command.arg("branch");
        command.arg("-D");
        command.arg(branch_name);
        command.current_dir(&self.repo_path);

        let output = log_command(command).output()?;
        handle_failure("delete_branch", &output)?;
        Ok(())
    }

    fn squash_merge(&self, author_name: &str, author_email: &str, branch_name: &str) -> Result<()> {
        let mut command = Command::new(&self.binary_name);
        command.arg("-c");
        command.arg(format!("user.name={author_name}"));
        command.arg("-c");
        command.arg(format!("user.email={author_email}"));
        command.arg("merge");
        command.arg("--squash");
        command.arg(branch_name);
        command.current_dir(&self.repo_path);

        let output = log_command(command).output()?;
        handle_failure("squash_merge", &output)
    }

    fn untracked_files(&self) -> Result<Vec<PathBuf>> {
        let mut command = Command::new(&self.binary_name);
        command.arg("ls-files");
        command.arg("--exclude-standard");
        command.arg("--others");
        command.current_dir(&self.repo_path);

        let output = log_command(command).output()?;
        handle_failure("untracked_files", &output)?;
        let (stdout, _) = output_text(&output);
        Ok(split_file_names(&stdout))
    }

    fn changed_files(&self) -> Result<Vec<PathBuf>> {
        let mut command = Command::new(&self.binary_name);
        command.arg("diff");
        command.arg("--name-only");
        command.current_dir(&self.repo_path);

        let output = log_command(command).output()?;
        handle_failure("changed_files", &output)?;
        let (stdout, _) = output_text(&output);
        Ok(split_file_names(&stdout))
    }

    fn get_current_tag(&self) -> Result<ReleaseTag> {
        let mut command = Command::new(&self.binary_name);
        command.arg("describe");
        command.arg("--tags");
        command.arg("--abbrev=0");
        command.current_dir(&self.repo_path);

        let output = command.output()?;
        handle_failure("get_current_tag", &output)?;
        let (stdout, _) = output_text(&output);
        ReleaseTag::from_str(stdout.trim())
    }
}

fn is_newline(c: char) -> bool {
    c == '\r' || c == '\n'
}

fn split_file_names(value: &str) -> Vec<PathBuf> {
    value
        .split(is_newline)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect::<Vec<_>>()
}

fn log_command(command: Command) -> Command {
    let mut message = String::new();
    if let Some(cwd) = command.get_current_dir() {
        write!(&mut message, "[in {cwd:?}]: ").unwrap();
    }
    message.push_str(command.get_program().to_str().expect("valid str"));
    for arg in command.get_args() {
        message.push(' ');
        message.push_str(arg.to_str().expect("valid str"));
    }
    debug!("{}", message);
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs};
    use tempfile::TempDir;

    fn bin_path(script: &'static str) -> PathBuf {
        env::current_dir()
            .expect("current_dir")
            .join("fake-cli")
            .join(script)
            .canonicalize()
            .expect("canonicalize")
    }
    fn cli(script: &'static str) -> GitCLI {
        GitCLI::with_binary(&PathBuf::from("/tmp"), &bin_path(script).to_string_lossy())
    }

    #[test]
    fn clone_to() {
        cli("git-clone")
            .clone_to(&PathBuf::from("/tmp"))
            .expect("successful invocation");
    }

    #[test]
    fn extract_commit_info() {
        let result = cli("git-extract-commit-info")
            .extract_commit_info("test_revision", CommitInfo::CommitHash)
            .expect("successful invocation");
        assert_eq!("success", result);
    }

    #[test]
    fn changed_files() {
        assert_eq!(
            vec![
                PathBuf::from("some/file"),
                PathBuf::from("some-other-file"),
                PathBuf::from("some/file with spaces.txt"),
            ],
            cli("git-changed-files")
                .changed_files()
                .expect("successful invocation")
        );
        assert_eq!(
            Vec::<PathBuf>::new(),
            cli("git-changed-files-empty")
                .changed_files()
                .expect("successful invocation")
        );
    }

    #[test]
    fn untracked_files() {
        assert_eq!(
            vec![
                PathBuf::from("some-untracked-file"),
                PathBuf::from("another-untracked-file"),
                PathBuf::from("some/file with spaces.txt"),
            ],
            cli("git-untracked-files")
                .untracked_files()
                .expect("successful invocation")
        );
        assert_eq!(
            Vec::<PathBuf>::new(),
            cli("git-untracked-files-empty")
                .untracked_files()
                .expect("successful invocation")
        );
    }

    #[test]
    fn get_head_revision() {
        assert_eq!(
            "some-commit-hash",
            cli("git-get-head-revision")
                .get_head_revision()
                .expect("successful invocation")
                .as_ref()
        );
    }

    #[test]
    fn stage() {
        cli("git-stage")
            .stage(&PathBuf::from("test-path"))
            .expect("successful invocation");
    }

    #[test]
    fn commit_on_behalf() {
        cli("git-commit-on-behalf")
            .commit_on_behalf(
                "Bot Name",
                "bot@example.com",
                "Some Author",
                "author@example.com",
                "Test message",
            )
            .expect("successful invocation");
    }

    #[test]
    fn commit() {
        cli("git-commit")
            .commit("Some Author", "author@example.com", "Test message")
            .expect("successful invocation");
    }

    #[test]
    fn rev_list() {
        assert_eq!(
            vec![
                CommitHash::from("second-commit"),
                CommitHash::from("initial-commit")
            ],
            cli("git-rev-list")
                .rev_list("start_inclusive", "end_exclusive", None)
                .expect("successful invocation")
        );
        assert_eq!(
            vec![
                CommitHash::from("third-commit"),
                CommitHash::from("second-commit"),
                CommitHash::from("initial-commit")
            ],
            cli("git-rev-list-path")
                .rev_list(
                    "start_inclusive",
                    "end_exclusive",
                    Some(&PathBuf::from("some-path"))
                )
                .expect("successful invocation")
        );
    }

    #[test]
    fn show() {
        assert_eq!(
            Commit {
                hash: "some-commit-hash".into(),
                author_name: "Some Author".into(),
                author_email: "author@example.com".into(),
                message_subject: "Some message subject".into(),
                message_body: "Message body\n  with multiple lines".into()
            },
            cli("git-show")
                .show("test_revision")
                .expect("successful invocation")
        );
    }

    #[test]
    fn hard_reset() {
        cli("git-reset-hard")
            .hard_reset("some-revision")
            .expect("successful invocation");
    }

    #[test]
    fn current_branch_name() {
        assert_eq!(
            "some-branch-name",
            cli("git-current-branch-name")
                .current_branch_name()
                .expect("successful invocation")
        );
    }

    #[test]
    fn create_branch() {
        cli("git-create-branch")
            .create_branch("test-branch-name", "test-revision")
            .expect("successful invocation");
    }

    #[test]
    fn delete_branch() {
        cli("git-delete-branch")
            .delete_branch("test-branch-name")
            .expect("successful invocation");
    }

    #[test]
    fn squash_merge() {
        cli("git-squash-merge")
            .squash_merge("some-dev", "some-email@example.com", "test-branch-name")
            .expect("successful invocation");
    }

    #[test]
    fn repository_root_check() {
        let tmp_dir = TempDir::new().unwrap();
        GitCLI::new(tmp_dir.path())
            .err()
            .expect("repository root check should fail");

        fs::create_dir(tmp_dir.path().join(".git")).unwrap();
        GitCLI::new(tmp_dir.path()).expect("repository root check should succeed");
    }

    #[test]
    fn repository_root_check_works_for_git_submodules() {
        let tmp_dir = TempDir::new().unwrap();
        GitCLI::new(tmp_dir.path())
            .err()
            .expect("repository root check should fail");

        fs::write(tmp_dir.path().join(".git"), "gitdir: some/fake/path").unwrap();
        GitCLI::new(tmp_dir.path()).expect("repository root check should succeed");
    }
}
