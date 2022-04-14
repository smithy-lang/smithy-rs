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
    /// The branch in aws-sdk-rust that commits will be mirrored to.
    #[clap(long, default_value = "next")]
    branch: String,
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
        &args.aws_doc_sdk_examples,
        &args.aws_sdk_rust,
        &args.smithy_rs,
    )?;

    sync.sync_aws_sdk_with_smithy_rs(&args.branch)
        .map_err(|e| e.context("The sync failed"))
}

struct Sync {
    aws_doc_sdk_examples: Box<dyn Git>,
    aws_sdk_rust: Box<dyn Git>,
    smithy_rs: Box<dyn Git>,
    smithy_rs_gradle: Box<dyn Gradle>,
    fs: Box<dyn Fs>,
}

impl Sync {
    fn new(
        aws_doc_sdk_examples_path: &Path,
        aws_sdk_rust_path: &Path,
        smithy_rs_path: &Path,
    ) -> Result<Self> {
        Self::new_with_git(
            aws_doc_sdk_examples_path,
            aws_sdk_rust_path,
            smithy_rs_path,
            |path| GitCLI::new(path).map(|g| Box::new(g) as Box<dyn Git>),
        )
    }

    fn new_with_git<G>(
        aws_doc_sdk_examples_path: &Path,
        aws_sdk_rust_path: &Path,
        smithy_rs_path: &Path,
        new_git_repo: G,
    ) -> Result<Self>
    where
        G: Fn(&Path) -> Result<Box<dyn Git>>,
    {
        Ok(Self {
            aws_doc_sdk_examples: new_git_repo(aws_doc_sdk_examples_path)?,
            aws_sdk_rust: new_git_repo(aws_sdk_rust_path)?,
            smithy_rs: new_git_repo(smithy_rs_path)?,
            smithy_rs_gradle: Box::new(GradleCLI::new(smithy_rs_path)) as Box<dyn Gradle>,
            fs: Box::new(DefaultFs::new()) as Box<dyn Fs>,
        })
    }

    /// Run through all commits made to `smithy-rs` since last sync and "replay" them onto `aws-sdk-rust`.
    fn sync_aws_sdk_with_smithy_rs(&self, branch: &str) -> Result<()> {
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
            "Successfully synced {} mirror commit(s) to aws-sdk-rust/{}. Don't forget to push them",
            commits.len(),
            branch
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
        self.fs
            .remove_dir_all_idempotent(&self.smithy_rs.path().join("aws/sdk/build"))
            .context(here!())?;
        self.smithy_rs_gradle.aws_sdk_clean().context(here!())?;
        self.smithy_rs_gradle
            .aws_sdk_assemble(&examples_revision)
            .context(here!())?;

        let build_artifact_path = self.smithy_rs.path().join("aws/sdk/build/aws-sdk");
        eprintln!("\tsuccessfully built the SDK in {:?}", start.elapsed());
        Ok(build_artifact_path)
    }

    /// Place the examples from aws-doc-sdk-examples into the correct place in smithy-rs
    /// to be included with the generated SDK.
    fn setup_examples(&self) -> Result<CommitHash> {
        let from = self
            .aws_doc_sdk_examples
            .path()
            .canonicalize()
            .context(here!())?;
        let from = from.join("rust_dev_preview");

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
            .recursive_copy(
                &from,
                &PathBuf::from("aws/sdk/examples"),
                Some(self.smithy_rs.path()),
            )
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

        // The '.' tells cp to copy the folder contents, not the folder
        let from_path = from_path.join(".");

        // This command uses absolute paths so working dir doesn't matter. Even so, we set
        // working dir to the dir this binary was run from because `run` expects one.
        // GitHub actions don't support current_dir so we use current_exe
        let exe_dir = std::env::current_exe().expect("can't access path of this exe");
        let working_dir = exe_dir.parent().expect("exe is not in a folder?");

        self.fs
            .recursive_copy(&from_path, to_path, Some(working_dir))
            .context(here!())?;

        eprintln!("\tsuccessfully copied built SDK");
        Ok(())
    }
}
