/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::fs::{DefaultFs, Fs};
use crate::git::{Commit, CommitHash, Git, GitCLI};
use crate::gradle::{Gradle, GradleCLI};
use crate::versions::{DefaultVersions, Versions, VersionsManifest};
use anyhow::{bail, Context, Result};
use smithy_rs_tool_common::macros::here;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const BOT_NAME: &str = "AWS SDK Rust Bot";
pub const BOT_EMAIL: &str = "aws-sdk-rust-primary@amazon.com";

pub struct Sync {
    aws_doc_sdk_examples: Box<dyn Git>,
    aws_sdk_rust: Box<dyn Git>,
    smithy_rs: Box<dyn Git>,
    smithy_rs_gradle: Box<dyn Gradle>,
    fs: Box<dyn Fs>,
    versions: Box<dyn Versions>,
}

impl Sync {
    pub fn new(
        aws_doc_sdk_examples_path: &Path,
        aws_sdk_rust_path: &Path,
        smithy_rs_path: &Path,
    ) -> Result<Self> {
        Ok(Self {
            aws_doc_sdk_examples: Box::new(GitCLI::new(aws_doc_sdk_examples_path)?),
            aws_sdk_rust: Box::new(GitCLI::new(aws_sdk_rust_path)?),
            smithy_rs: Box::new(GitCLI::new(smithy_rs_path)?),
            smithy_rs_gradle: Box::new(GradleCLI::new(smithy_rs_path)) as Box<dyn Gradle>,
            fs: Box::new(DefaultFs::new()) as Box<dyn Fs>,
            versions: Box::new(DefaultVersions::new()) as Box<dyn Versions>,
        })
    }

    pub fn new_with(
        aws_doc_sdk_examples: impl Git + 'static,
        aws_sdk_rust: impl Git + 'static,
        smithy_rs: impl Git + 'static,
        smithy_rs_gradle: impl Gradle + 'static,
        fs: impl Fs + 'static,
        versions: impl Versions + 'static,
    ) -> Self {
        Self {
            aws_doc_sdk_examples: Box::new(aws_doc_sdk_examples),
            aws_sdk_rust: Box::new(aws_sdk_rust),
            smithy_rs: Box::new(smithy_rs),
            smithy_rs_gradle: Box::new(smithy_rs_gradle),
            fs: Box::new(fs),
            versions: Box::new(versions),
        }
    }

    pub fn sync(&self) -> Result<()> {
        let versions = self
            .versions
            .load(self.aws_sdk_rust.path())
            .context("load versions.toml")?;

        self.sync_smithy_rs(&versions).context("sync smithy-rs")?;
        self.sync_examples(&versions).context("sync examples")?;

        Ok(())
    }

    /// Run through all commits made to `smithy-rs` since last sync and "replay" them onto `aws-sdk-rust`.
    fn sync_smithy_rs(&self, versions: &VersionsManifest) -> Result<()> {
        eprintln!(
            "Checking for smithy-rs commits in range HEAD..{}",
            versions.smithy_rs_revision
        );
        let mut commits = self
            .smithy_rs
            .rev_list("HEAD", versions.smithy_rs_revision.as_ref(), None)
            .context(here!())?;
        commits.reverse(); // Order the revs from earliest to latest

        if commits.is_empty() {
            eprintln!("There are no new commits to be applied, have a nice day.");
            return Ok(());
        }

        eprintln!("Syncing {} commit(s)...", commits.len());

        // Run through all the new commits, syncing them one by one
        for (i, rev) in commits.iter().enumerate() {
            let commit = self
                .smithy_rs
                .show(rev.as_ref())
                .with_context(|| format!("couldn't find commit {} in smithy-rs", rev))?;

            eprintln!("[{}]\tsyncing {}...", i + 1, rev);
            self.smithy_rs
                .hard_reset(commit.hash.as_ref())
                .with_context(|| format!("failed to reset to {} in smithy-rs", rev,))?;

            self.copy_original_examples().context(here!())?;
            self.build_and_copy_sdk(&versions.aws_doc_sdk_examples_revision)
                .context("failed to generate the SDK")?;
            self.commit_sdk_changes("[smithy-rs]", &commit)
                .context("couldn't commit SDK changes to aws-sdk-rust")?;
        }

        eprintln!(
            "Successfully synced {} smithy-rs commit(s) to aws-sdk-rust",
            commits.len(),
        );

        Ok(())
    }

    /// Aggregate the commits made to Rust SDK examples into one while maintaining attribution.
    /// We squash all the example commits since the examples repo does merges rather than squash merges,
    /// and we prefer squash merges in aws-sdk-rust.
    fn sync_examples(&self, versions: &VersionsManifest) -> Result<()> {
        // Only consider revisions on the `rust_dev_preview/` directory since
        // copying over changes to other language examples is pointless
        let example_revisions = self
            .aws_doc_sdk_examples
            .rev_list(
                "HEAD",
                versions.aws_doc_sdk_examples_revision.as_ref(),
                Some(&PathBuf::from("rust_dev_preview")),
            )
            .context(here!())?;
        if example_revisions.is_empty() {
            eprintln!("No example changes to copy over.");
            return Ok(());
        }
        let examples_head = example_revisions.iter().cloned().next().unwrap();

        let from = self.aws_doc_sdk_examples.path().join("rust_dev_preview");

        eprintln!("\tcleaning examples...");
        self.fs
            .remove_dir_all_idempotent(&self.smithy_rs.path().join("aws/sdk/examples"))
            .context(here!())?;

        eprintln!(
            "\tcopying examples from {:?} to 'smithy-rs/aws/sdk/examples'...",
            from
        );
        self.fs
            .recursive_copy(&from, &self.smithy_rs.path().join("aws/sdk/examples"))
            .context(here!())?;
        self.fs
            .remove_dir_all_idempotent(&self.smithy_rs.path().join("aws/sdk/examples/.cargo"))
            .context(here!())?;
        self.fs
            .remove_file(&self.smithy_rs.path().join("aws/sdk/examples/Cargo.toml"))
            .context(here!())?;

        self.build_and_copy_sdk(&examples_head).context(here!())?;
        self.aws_sdk_rust
            .stage(&PathBuf::from("."))
            .context(here!())?;

        let example_commits: Vec<Commit> = example_revisions
            .iter()
            .map(|commit_hash| self.aws_doc_sdk_examples.show(commit_hash.as_ref()))
            .collect::<Result<_, _>>()?;
        let commit_message = Self::format_example_commit_message(&example_commits);
        self.aws_sdk_rust
            .commit(BOT_NAME, BOT_EMAIL, &commit_message)?;
        Ok(())
    }

    fn format_example_commit_message(example_commits: &[Commit]) -> String {
        let mut commit_message =
            "[examples] Sync SDK examples from `awsdocs/aws-doc-sdk-examples`\n\n".to_string();
        commit_message.push_str("Includes commit(s):\n");
        for commit in example_commits.iter().rev() {
            commit_message.push_str("  ");
            commit_message.push_str(commit.hash.as_ref());
            commit_message.push('\n');
        }
        commit_message.push('\n');

        let authors: BTreeSet<String> = example_commits
            .iter()
            .map(|c| format!("{} <{}>", c.author_name, c.author_email))
            .collect();
        for author in authors {
            commit_message.push_str(&format!("Co-authored-by: {}\n", author));
        }
        commit_message
    }

    /// Generate an SDK and copy it into `aws-sdk-rust`.
    fn build_and_copy_sdk(&self, aws_doc_sdk_examples_revision: &CommitHash) -> Result<()> {
        eprintln!("\tbuilding the SDK...");
        let start = Instant::now();

        // The output of running these commands isn't logged anywhere unless they fail
        self.smithy_rs_gradle.aws_sdk_clean().context(here!())?;
        self.smithy_rs_gradle
            .aws_sdk_assemble(aws_doc_sdk_examples_revision)
            .context(here!())?;

        let build_artifact_path = self.smithy_rs.path().join("aws/sdk/build/aws-sdk");
        eprintln!("\tsuccessfully generated the SDK in {:?}", start.elapsed());

        self.clean_out_existing_sdk()
            .context("couldn't clean out existing SDK from aws-sdk-rust")?;

        // Check that we aren't generating any files that we've marked as "handwritten"
        let handwritten_files_in_generated_sdk_folder = self
            .fs
            .find_handwritten_files_and_folders(self.aws_sdk_rust.path(), &build_artifact_path)?;
        if !handwritten_files_in_generated_sdk_folder.is_empty() {
            bail!(
                    "found one or more 'handwritten' files/folders in generated code: {:#?}\nhint: if this file is newly generated, remove it from .handwritten",
                    handwritten_files_in_generated_sdk_folder
                );
        }

        self.copy_sdk(&build_artifact_path, self.aws_sdk_rust.path())?;
        Ok(())
    }

    /// Copies current examples in aws-sdk-rust back into smithy-rs.
    fn copy_original_examples(&self) -> Result<()> {
        eprintln!("\tcleaning examples...");
        self.fs
            .remove_dir_all_idempotent(&self.smithy_rs.path().join("aws/sdk/examples"))
            .context(here!())?;

        let from = self.aws_sdk_rust.path().join("examples");
        eprintln!(
            "\tcopying examples from {:?} to 'smithy-rs/aws/sdk/examples'...",
            from
        );
        self.fs
            .recursive_copy(&from, &self.smithy_rs.path().join("aws/sdk/examples"))
            .context(here!())?;
        Ok(())
    }

    /// Commit the changes to aws-sdk-rust reflecting the info from a commit in another repository.
    fn commit_sdk_changes(&self, prefix: &str, based_on_commit: &Commit) -> Result<()> {
        if self.aws_sdk_rust.has_changes().context(here!())? {
            eprintln!("\tcommitting...");

            self.aws_sdk_rust
                .stage(&PathBuf::from("."))
                .context(here!())?;

            self.aws_sdk_rust.commit_on_behalf(
                BOT_NAME,
                BOT_EMAIL,
                &based_on_commit.author_name,
                &based_on_commit.author_email,
                &format!("{} {}", prefix, &based_on_commit.message()),
            )?;

            let commit_hash = self.aws_sdk_rust.get_head_revision()?;
            eprintln!("\tsuccessfully committed {}", commit_hash);
        } else {
            eprintln!(
                "No changes to the SDK found for commit: {}. Skipping.",
                based_on_commit.hash
            );
        }
        Ok(())
    }

    /// Delete any current SDK files in aws-sdk-rust. Run this before copying over new files.
    fn clean_out_existing_sdk(&self) -> Result<()> {
        eprintln!("\tcleaning out previously built SDK...");
        let start = Instant::now();
        self.fs
            .delete_all_generated_files_and_folders(self.aws_sdk_rust.path())
            .context(here!())?;
        eprintln!(
            "\tsuccessfully cleaned out previously built SDK in {:?}",
            start.elapsed()
        );
        Ok(())
    }

    /// Recursively copy all files and folders from the smithy-rs build artifacts folder
    /// to the aws-sdk-rust repo folder. Paths passed in must be absolute.
    fn copy_sdk(&self, from_path: &Path, to_path: &Path) -> Result<()> {
        eprintln!("\tcopying built SDK...");

        assert!(
            from_path.is_absolute(),
            "expected absolute from_path but got: {:?}",
            from_path
        );
        assert!(
            to_path.is_absolute(),
            "expected absolute to_path but got: {:?}",
            to_path
        );

        // The '.' copies the folder contents rather than the folder
        self.fs
            .recursive_copy(&from_path.join("."), to_path)
            .context(here!())?;

        eprintln!("\tsuccessfully copied built SDK");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::MockFs;
    use crate::git::MockGit;
    use crate::gradle::MockGradle;
    use crate::versions::MockVersions;

    // Wish this was in std...
    fn trim_indent(value: &str) -> String {
        let lines: Vec<&str> = value.split('\n').collect();
        let min_indent = value
            .split('\n')
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.find(|c: char| !c.is_ascii_whitespace()).unwrap())
            .min()
            .unwrap();
        let mut result = String::new();
        for line in lines {
            if line.len() > min_indent {
                result.push_str(&line[min_indent..]);
            } else {
                result.push_str(line);
            }
            result.push('\n');
        }
        result
    }

    #[test]
    fn test_format_example_commit() {
        let commits = vec![
            Commit {
                hash: "hash3".into(),
                author_name: "Some Dev".into(),
                author_email: "somedev@example.com".into(),
                message_subject: "Some commit subject".into(),
                message_body: "".into(),
            },
            Commit {
                hash: "hash2".into(),
                author_name: "Some Dev".into(),
                author_email: "somedev@example.com".into(),
                message_subject: "Doesn't matter".into(),
                message_body: "".into(),
            },
            Commit {
                hash: "hash1".into(),
                author_name: "Another Dev".into(),
                author_email: "anotherdev@example.com".into(),
                message_subject: "Another commit subject".into(),
                message_body: "This one has a body\n\n- bullet\n- bullet\n\nmore".into(),
            },
        ];

        let actual = Sync::format_example_commit_message(&commits);
        pretty_assertions::assert_str_eq!(
            trim_indent(
                r#"
                [examples] Sync SDK examples from `awsdocs/aws-doc-sdk-examples`

                Includes commit(s):
                  hash1
                  hash2
                  hash3

                Co-authored-by: Another Dev <anotherdev@example.com>
                Co-authored-by: Some Dev <somedev@example.com>"#
            ),
            format!("\n{}", actual)
        )
    }

    // When there are no changes to the SDK, it should NOT commit anything
    #[test]
    fn test_commit_sdk_changes_no_changes() {
        let mut aws_sdk_rust = MockGit::new();

        // Say there are no changes when asked
        aws_sdk_rust
            .expect_has_changes()
            .once()
            .returning(|| Ok(false));

        // No staging or committing should occur
        aws_sdk_rust.expect_stage().never();
        aws_sdk_rust.expect_commit_on_behalf().never();

        let sync = Sync::new_with(
            MockGit::new(),
            aws_sdk_rust,
            MockGit::new(),
            MockGradle::new(),
            MockFs::new(),
            MockVersions::new(),
        );
        assert!(sync
            .commit_sdk_changes(
                "[test]",
                &Commit {
                    hash: "hash".into(),
                    author_name: "Some Dev".into(),
                    author_email: "somedev@example.com".into(),
                    message_subject: "Some commit".into(),
                    message_body: "".into(),
                },
            )
            .is_ok());
    }

    // When there are changes to the SDK, it should commit them
    #[test]
    fn test_commit_sdk_changes_changes() {
        let mut aws_sdk_rust = MockGit::new();

        // Say there are changes when asked
        aws_sdk_rust
            .expect_has_changes()
            .once()
            .returning(|| Ok(true));

        // Expect staging and a commit
        aws_sdk_rust
            .expect_stage()
            .withf(|p| p == &PathBuf::from("."))
            .once()
            .returning(|_| Ok(()));
        aws_sdk_rust
            .expect_commit_on_behalf()
            .withf(|name, email, author_name, author_email, message| {
                name == BOT_NAME
                    && email == BOT_EMAIL
                    && author_name == "Some Dev"
                    && author_email == "somedev@example.com"
                    && message == "[test] Some commit"
            })
            .once()
            .returning(|_, _, _, _, _| Ok(()));
        aws_sdk_rust
            .expect_get_head_revision()
            .once()
            .returning(|| Ok(CommitHash::from("new-commit-hash")));

        let sync = Sync::new_with(
            MockGit::new(),
            aws_sdk_rust,
            MockGit::new(),
            MockGradle::new(),
            MockFs::new(),
            MockVersions::new(),
        );
        assert!(sync
            .commit_sdk_changes(
                "[test]",
                &Commit {
                    hash: "hash".into(),
                    author_name: "Some Dev".into(),
                    author_email: "somedev@example.com".into(),
                    message_subject: "Some commit".into(),
                    message_body: "".into(),
                },
            )
            .is_ok());
    }
}
