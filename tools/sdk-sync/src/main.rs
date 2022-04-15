/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

mod fs;
mod git;
mod gradle;

use anyhow::{bail, Context, Result};
use clap::Parser;
use fs::{DefaultFs, Fs};
use git::{Commit, CommitHash, Git, GitCLI};
use gradle::{Gradle, GradleCLI};
use smithy_rs_tool_common::macros::here;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// A CLI tool to replay commits from smithy-rs, generate code, and commit that code to aws-rust-sdk.
#[derive(Parser, Debug)]
#[clap(name = "smithy-rs-sync")]
struct Args {
    /// The path to the smithy-rs repo folder.
    #[clap(long, parse(from_os_str))]
    smithy_rs: PathBuf,
    /// The path to the aws-sdk-rust folder.
    #[clap(long, parse(from_os_str))]
    aws_sdk_rust: PathBuf,
    /// Path to the aws-doc-sdk-examples repository.
    #[clap(long, parse(from_os_str))]
    aws_doc_sdk_examples: PathBuf,
}

const BOT_NAME: &str = "AWS SDK Rust Bot";
const BOT_EMAIL: &str = "aws-sdk-rust-primary@amazon.com";
const BOT_COMMIT_PREFIX: &str = "[autosync]";
const COMMIT_HASH_FILENAME: &str = ".smithyrs-githash";

/// This tool syncs codegen changes from smithy-rs, examples changes from aws-doc-sdk-examples,
/// and any additional (optional) model changes into aws-sdk-rust.
///
/// The goal is for this tool to be fully tested via `cargo test`, but to execute it locally,
/// you'll need:
/// - Local copy of aws-doc-sdk-examples repo
/// - Local copy of aws-sdk-rust repo
/// - Local copy of smithy-rs repo
/// - A Unix-ey system (for the `cp` and `rf` commands to work)
/// - Java Runtime Environment v11 (in order to run gradle commands)
///
/// ```sh
/// cargo run -- \
///   --aws-doc-sdk-examples /Users/zhessler/Documents/aws-doc-sdk-examples \
///   --aws-sdk-rust /Users/zhessler/Documents/aws-sdk-rust-test \
///   --smithy-rs /Users/zhessler/Documents/smithy-rs-test
/// ```
fn main() -> Result<()> {
    let args = Args::parse();

    let sync = Sync::new(
        &args.aws_doc_sdk_examples.canonicalize().context(here!())?,
        &args.aws_sdk_rust.canonicalize().context(here!())?,
        &args.smithy_rs.canonicalize().context(here!())?,
    )?;

    sync.sync().map_err(|e| e.context("The sync failed"))
}

struct Sync {
    aws_doc_sdk_examples: Box<dyn Git>,
    aws_sdk_rust: Box<dyn Git>,
    smithy_rs: Box<dyn Git>,
    smithy_rs_gradle: Box<dyn Gradle>,
    fs: Box<dyn Fs>,
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
        })
    }

    #[cfg(test)]
    fn new_with(
        aws_doc_sdk_examples: impl Git + 'static,
        aws_sdk_rust: impl Git + 'static,
        smithy_rs: impl Git + 'static,
        smithy_rs_gradle: impl Gradle + 'static,
        fs: impl Fs + 'static,
    ) -> Self {
        Self {
            aws_doc_sdk_examples: Box::new(aws_doc_sdk_examples),
            aws_sdk_rust: Box::new(aws_sdk_rust),
            smithy_rs: Box::new(smithy_rs),
            smithy_rs_gradle: Box::new(smithy_rs_gradle),
            fs: Box::new(fs),
        }
    }

    /// Run through all commits made to `smithy-rs` since last sync and "replay" them onto `aws-sdk-rust`.
    pub fn sync(&self) -> Result<()> {
        // Check repo that we're going to be moving the code into to see what commit it was last synced with
        let last_synced_commit = self
            .get_last_synced_commit()
            .context("couldn't get last synced commit")?;
        let commits = self
            .commits_to_be_applied(&last_synced_commit)
            .context("couldn't build list of commits that need to be synced")?;

        if commits.is_empty() {
            eprintln!("There are no new commits to be applied, have a nice day.");
            return Ok(());
        }

        eprintln!("Syncing {} un-synced commit(s)...", commits.len());

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

            let build_artifacts = self.build_sdk().context("couldn't build SDK")?;
            self.clean_out_existing_sdk()
                .context("couldn't clean out existing SDK from aws-sdk-rust")?;

            // Check that we aren't generating any files that we've marked as "handwritten"
            let handwritten_files_in_generated_sdk_folder = self
                .fs
                .find_handwritten_files_and_folders(self.aws_sdk_rust.path(), &build_artifacts)?;
            if !handwritten_files_in_generated_sdk_folder.is_empty() {
                bail!(
                    "found one or more 'handwritten' files/folders in generated code: {:#?}\nhint: if this file is newly generated, remove it from .handwritten",
                    handwritten_files_in_generated_sdk_folder
                );
            }

            self.copy_sdk(&build_artifacts, self.aws_sdk_rust.path())?;
            self.create_mirror_commit(&commit)
                .context("couldn't commit SDK changes to aws-sdk-rust")?;
        }

        eprintln!(
            "Successfully synced {} mirror commit(s) to aws-sdk-rust",
            commits.len(),
        );

        Ok(())
    }

    /// Read the file from aws-sdk-rust that tracks the last smithy-rs commit it was synced with.
    /// Returns the hash of that commit.
    fn get_last_synced_commit(&self) -> Result<CommitHash> {
        // TODO: Replace with versions.toml
        let path = self.aws_sdk_rust.path().join(COMMIT_HASH_FILENAME);
        Ok(CommitHash::from(
            self.fs
                .read_to_string(&path)
                .with_context(|| {
                    format!("couldn't get commit hash from file at '{}'", path.display())
                })?
                .trim()
                .to_string(),
        ))
    }

    /// Starting from a given commit, walk the tree to its `HEAD` in order to build a list of commits that we'll
    /// need to sync. If you don't see the commits you're expecting, make sure the repo is up to date.
    /// This function doesn't include the `since_commit` in the list since that commit was synced last time
    /// this tool was run.
    fn commits_to_be_applied(&self, since_commit: &CommitHash) -> Result<Vec<CommitHash>> {
        eprintln!(
            "Checking for smithy-rs commits in range HEAD..{}",
            since_commit
        );
        let mut commits = self
            .smithy_rs
            .rev_list("HEAD", since_commit.as_ref())
            .context(here!())?;
        commits.reverse(); // Order the revs from earliest to latest
        Ok(commits)
    }

    /// Run the necessary commands to build the SDK. On success, returns the path to the folder containing
    /// the build artifacts.
    fn build_sdk(&self) -> Result<PathBuf> {
        let examples_revision = self.setup_examples().context(here!())?;

        eprintln!("\tbuilding the SDK...");
        let start = Instant::now();

        // The output of running these commands isn't logged anywhere unless they fail
        self.smithy_rs_gradle.aws_sdk_clean().context(here!())?;
        self.smithy_rs_gradle
            .aws_sdk_assemble(&examples_revision)
            .context(here!())?;

        let build_artifact_path = self.smithy_rs.path().join("aws/sdk/build/aws-sdk");
        eprintln!("\tsuccessfully generated the SDK in {:?}", start.elapsed());
        Ok(build_artifact_path)
    }

    /// Place the examples from aws-doc-sdk-examples into the correct place in smithy-rs
    /// to be included with the generated SDK.
    fn setup_examples(&self) -> Result<CommitHash> {
        let from = self.aws_doc_sdk_examples.path().join("rust_dev_preview");

        let examples_revision = self
            .aws_doc_sdk_examples
            .get_head_revision()
            .context(here!())?;

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
        Ok(examples_revision)
    }

    /// Create a "mirror" commit. Works by reading a smithy-rs commit and then using the info
    /// attached to it to create a commit in aws-sdk-rust. This also updates the `.smithyrs-githash`
    /// file with the hash of `based_on_commit`.
    ///
    /// *NOTE: If you're wondering why `git2` is used elsewhere in this tool but not in this function,
    /// it's due to strange, undesired behavior that occurs. For every commit, something
    /// happened that created leftover staged and unstaged changes. When the unstaged changes
    /// were staged, they cancelled out the first set of staged changes. I don't know why this
    /// happened and if you think you can fix it, you can check out the previous version of the*
    /// tool in [this PR](https://github.com/awslabs/smithy-rs/pull/1042)
    fn create_mirror_commit(&self, based_on_commit: &Commit) -> Result<()> {
        eprintln!("\tcreating mirror commit...");

        self.aws_sdk_rust
            .stage(&PathBuf::from("."))
            .context(here!())?;

        self.aws_sdk_rust.commit_on_behalf(
            BOT_NAME,
            BOT_EMAIL,
            &based_on_commit.author_name,
            &based_on_commit.author_email,
            &format!("{} {}", BOT_COMMIT_PREFIX, &based_on_commit.message()),
        )?;

        let commit_hash = self.aws_sdk_rust.get_head_revision()?;
        eprintln!("\tsuccessfully created mirror commit {}", commit_hash);

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
    use fs::MockFs;
    use git::MockGit;
    use gradle::MockGradle;
    use mockall::{predicate::*, Sequence};

    fn set_path(mock_git: &mut MockGit, path: &str) {
        mock_git.expect_path().return_const(PathBuf::from(path));
    }

    fn set_head(repo: &mut MockGit, head: &'static str) {
        repo.expect_get_head_revision()
            .returning(move || Ok(CommitHash::from(head)));
    }

    fn expect_show_commit(repo: &mut MockGit, seq: &mut Sequence, commit: Commit) {
        let hash = commit.hash.as_ref().to_string();
        repo.expect_show()
            .withf(move |h| hash == h)
            .once()
            .in_sequence(seq)
            .returning(move |_| Ok(commit.clone()));
    }

    fn expect_hard_reset(repo: &mut MockGit, seq: &mut Sequence, hash: &str) {
        let hash = hash.to_string();
        repo.expect_hard_reset()
            .withf(move |h| h == hash)
            .once()
            .in_sequence(seq)
            .returning(|_| Ok(()));
    }

    fn expect_stage(repo: &mut MockGit, seq: &mut Sequence, path: &'static str) {
        repo.expect_stage()
            .withf(move |p| p.to_string_lossy() == path)
            .once()
            .in_sequence(seq)
            .returning(|_| Ok(()));
    }

    #[derive(Default)]
    struct Mocks {
        aws_doc_sdk_examples: MockGit,
        aws_sdk_rust: MockGit,
        smithy_rs: MockGit,
        smithy_rs_gradle: MockGradle,
        fs: MockFs,
    }

    impl Mocks {
        fn into_sync(self) -> Sync {
            Sync::new_with(
                self.aws_doc_sdk_examples,
                self.aws_sdk_rust,
                self.smithy_rs,
                self.smithy_rs_gradle,
                self.fs,
            )
        }

        fn set_smithyrs_githash(&mut self, hash: &'static str) {
            self.fs
                .expect_read_to_string()
                .withf(|path| path.to_string_lossy() == "/p2/aws-sdk-rust/.smithyrs-githash")
                .once()
                .returning(|_| Ok(hash.to_string()));
        }

        fn set_smithyrs_commits_to_sync(
            &mut self,
            previous_synced_commit: &'static str,
            hashes: &'static [&'static str],
        ) {
            self.smithy_rs
                .expect_rev_list()
                .with(eq("HEAD"), eq(previous_synced_commit))
                .once()
                .returning(|_, _| Ok(hashes.iter().map(|&hash| CommitHash::from(hash)).collect()));
        }

        fn expect_remove_dir_all_idempotent(&mut self, seq: &mut Sequence, path: &'static str) {
            self.fs
                .expect_remove_dir_all_idempotent()
                .withf(move |p| p.to_string_lossy() == path)
                .once()
                .in_sequence(seq)
                .returning(|_| Ok(()));
        }

        fn expect_recursive_copy(
            &mut self,
            seq: &mut Sequence,
            source: &'static str,
            dest: &'static str,
        ) {
            self.fs
                .expect_recursive_copy()
                .withf(move |src, dst| {
                    src.to_string_lossy() == source && dst.to_string_lossy() == dest
                })
                .once()
                .in_sequence(seq)
                .returning(|_, _| Ok(()));
        }

        fn expect_remove_file(&mut self, seq: &mut Sequence, path: &'static str) {
            self.fs
                .expect_remove_file()
                .withf(move |p| p.to_string_lossy() == path)
                .once()
                .in_sequence(seq)
                .returning(|_| Ok(()));
        }

        fn expect_build(&mut self, seq: &mut Sequence, examples_head: &'static str) {
            self.smithy_rs_gradle
                .expect_aws_sdk_clean()
                .once()
                .in_sequence(seq)
                .returning(|| Ok(()));
            self.smithy_rs_gradle
                .expect_aws_sdk_assemble()
                .withf(move |examples_revision| examples_revision.as_ref() == examples_head)
                .once()
                .in_sequence(seq)
                .returning(|_| Ok(()));
        }

        fn expect_delete_all_generated_files_and_folders(
            &mut self,
            seq: &mut Sequence,
            sdk_path: &'static str,
        ) {
            self.fs
                .expect_delete_all_generated_files_and_folders()
                .withf(move |p| p.to_string_lossy() == sdk_path)
                .once()
                .in_sequence(seq)
                .returning(|_| Ok(()));
        }

        fn expect_find_handwritten_files_and_folders(
            &mut self,
            seq: &mut Sequence,
            sdk_path: &'static str,
            artifacts_path: &'static str,
            files: &'static [&'static str],
        ) {
            self.fs
                .expect_find_handwritten_files_and_folders()
                .withf(move |aws_sdk_p, artifacts_p| {
                    aws_sdk_p.to_string_lossy() == sdk_path
                        && artifacts_p.to_string_lossy() == artifacts_path
                })
                .once()
                .in_sequence(seq)
                .returning(move |_, _| Ok(files.iter().map(PathBuf::from).collect()));
        }
    }

    fn expect_successful_sync(
        mocks: &mut Mocks,
        seq: &mut Sequence,
        commit: Commit,
        expected_commit_message: &str,
    ) {
        expect_show_commit(&mut mocks.smithy_rs, seq, commit.clone());
        expect_hard_reset(&mut mocks.smithy_rs, seq, commit.hash.as_ref());

        // Examples sync
        mocks.expect_remove_dir_all_idempotent(seq, "/p2/smithy-rs/aws/sdk/examples");
        mocks.expect_recursive_copy(
            seq,
            "/p2/aws-doc-sdk-examples/rust_dev_preview",
            "/p2/smithy-rs/aws/sdk/examples",
        );
        mocks.expect_remove_dir_all_idempotent(seq, "/p2/smithy-rs/aws/sdk/examples/.cargo");
        mocks.expect_remove_file(seq, "/p2/smithy-rs/aws/sdk/examples/Cargo.toml");

        // Codegen
        mocks.expect_build(seq, "examples-head");
        mocks.expect_delete_all_generated_files_and_folders(seq, "/p2/aws-sdk-rust");
        mocks.expect_find_handwritten_files_and_folders(
            seq,
            "/p2/aws-sdk-rust",
            "/p2/smithy-rs/aws/sdk/build/aws-sdk",
            &[], // no handwritten files found
        );
        mocks.expect_recursive_copy(
            seq,
            "/p2/smithy-rs/aws/sdk/build/aws-sdk/.",
            "/p2/aws-sdk-rust",
        );

        // Commit generated SDK
        expect_stage(&mut mocks.aws_sdk_rust, seq, ".");
        let expected_commit_message = expected_commit_message.to_string();
        mocks
            .aws_sdk_rust
            .expect_commit_on_behalf()
            .withf(
                move |bot_name, bot_email, author_name, author_email, message| {
                    bot_name == BOT_NAME
                        && bot_email == BOT_EMAIL
                        && author_name == commit.author_name
                        && author_email == commit.author_email
                        && message == expected_commit_message
                },
            )
            .once()
            .returning(|_, _, _, _, _| Ok(()));
        mocks
            .aws_sdk_rust
            .expect_get_head_revision()
            .once()
            .returning(|| Ok(CommitHash::from("newly-synced-hash")));
    }

    #[test]
    fn mocked_e2e() {
        let mut mocks = Mocks::default();
        let mut seq = Sequence::new();

        set_path(&mut mocks.aws_doc_sdk_examples, "/p2/aws-doc-sdk-examples");
        set_path(&mut mocks.aws_sdk_rust, "/p2/aws-sdk-rust");
        set_path(&mut mocks.smithy_rs, "/p2/smithy-rs");

        mocks.set_smithyrs_githash("some-previous-commit-hash");
        mocks.set_smithyrs_commits_to_sync(
            "some-previous-commit-hash",
            &["hash-newest", "hash-oldest"],
        );
        set_head(&mut mocks.aws_doc_sdk_examples, "examples-head");

        expect_successful_sync(
            &mut mocks,
            &mut seq,
            Commit {
                hash: "hash-oldest".into(),
                author_name: "Some Dev".into(),
                author_email: "somedev@example.com".into(),
                message_subject: "Some commit subject".into(),
                message_body: "".into(),
            },
            "[autosync] Some commit subject",
        );
        expect_successful_sync(
            &mut mocks,
            &mut seq,
            Commit {
                hash: "hash-newest".into(),
                author_name: "Another Dev".into(),
                author_email: "anotherdev@example.com".into(),
                message_subject: "Another commit subject".into(),
                message_body: "This one has a body\n\n- bullet\n- bullet\n\nmore".into(),
            },
            "[autosync] Another commit subject\n\nThis one has a body\n\n- bullet\n- bullet\n\nmore",
        );

        let sync = mocks.into_sync();
        sync.sync().expect("success");
    }
}
