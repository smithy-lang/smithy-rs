/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

mod fs;
mod git;
mod gradle;
mod versions;

use anyhow::{bail, Context, Result};
use clap::Parser;
use fs::{DefaultFs, Fs};
use git::{Commit, CommitHash, Git, GitCLI};
use gradle::{Gradle, GradleCLI};
use smithy_rs_tool_common::macros::here;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;
use versions::{DefaultVersions, Versions, VersionsManifest};

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

    #[cfg(test)]
    fn new_with(
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
            self.commit("[smithy-rs]", &commit)
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
    fn commit(&self, prefix: &str, based_on_commit: &Commit) -> Result<()> {
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
    use crate::versions::{MockVersions, VersionsManifest};
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
        versions: MockVersions,
    }

    impl Mocks {
        fn into_sync(self) -> Sync {
            Sync::new_with(
                self.aws_doc_sdk_examples,
                self.aws_sdk_rust,
                self.smithy_rs,
                self.smithy_rs_gradle,
                self.fs,
                self.versions,
            )
        }

        fn set_smithyrs_commits_to_sync(
            &mut self,
            previous_synced_commit: &'static str,
            hashes: &'static [&'static str],
        ) {
            self.smithy_rs
                .expect_rev_list()
                .withf(move |begin, end, path| {
                    begin == "HEAD" && end == previous_synced_commit && path.is_none()
                })
                .once()
                .returning(|_, _, _| {
                    Ok(hashes.iter().map(|&hash| CommitHash::from(hash)).collect())
                });
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

        fn expect_build(&mut self, seq: &mut Sequence, examples_head: &str) {
            self.smithy_rs_gradle
                .expect_aws_sdk_clean()
                .once()
                .in_sequence(seq)
                .returning(|| Ok(()));
            let examples_head = examples_head.to_string();
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

    fn expect_codegen(mocks: &mut Mocks, seq: &mut Sequence, examples_head: &str) {
        mocks.expect_build(seq, examples_head);
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
    }

    fn expect_successful_smithyrs_sync(
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
            "/p2/aws-sdk-rust/examples",
            "/p2/smithy-rs/aws/sdk/examples",
        );

        // Codegen
        expect_codegen(mocks, seq, "old-examples-hash");

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

    fn expect_successful_example_sync(
        mocks: &mut Mocks,
        seq: &mut Sequence,
        old_examples_hash: &'static str,
        example_commits: &[Commit],
    ) {
        // Example revision discovery
        {
            let example_commits = example_commits.to_vec();
            mocks
                .aws_doc_sdk_examples
                .expect_rev_list()
                .withf(move |begin, end, path| {
                    begin == "HEAD"
                        && end == old_examples_hash
                        && *path == Some(&PathBuf::from("rust_dev_preview"))
                })
                .once()
                .in_sequence(seq)
                .returning(move |_, _, _| {
                    Ok(example_commits.iter().cloned().map(|c| c.hash).collect())
                });
        }

        // Example cleaning
        mocks.expect_remove_dir_all_idempotent(seq, "/p2/smithy-rs/aws/sdk/examples");
        mocks.expect_recursive_copy(
            seq,
            "/p2/aws-doc-sdk-examples/rust_dev_preview",
            "/p2/smithy-rs/aws/sdk/examples",
        );
        mocks.expect_remove_dir_all_idempotent(seq, "/p2/smithy-rs/aws/sdk/examples/.cargo");
        mocks.expect_remove_file(seq, "/p2/smithy-rs/aws/sdk/examples/Cargo.toml");

        // Codegen
        let examples_head = example_commits.iter().next().unwrap().hash.clone();
        expect_codegen(mocks, seq, examples_head.as_ref());

        // Commit generated SDK
        expect_stage(&mut mocks.aws_sdk_rust, seq, ".");
        for commit in example_commits {
            expect_show_commit(&mut mocks.aws_doc_sdk_examples, seq, commit.clone());
        }
        mocks
            .aws_sdk_rust
            .expect_commit()
            .withf(|name, email, message| {
                name == BOT_NAME
                    && email == BOT_EMAIL
                    && message.starts_with("[examples] Sync SDK examples")
            })
            .once()
            .in_sequence(seq)
            .returning(|_, _, _| Ok(()));
    }

    #[test]
    fn mocked_e2e() {
        let mut mocks = Mocks::default();
        let mut seq = Sequence::new();

        set_path(&mut mocks.aws_doc_sdk_examples, "/p2/aws-doc-sdk-examples");
        set_path(&mut mocks.aws_sdk_rust, "/p2/aws-sdk-rust");
        set_path(&mut mocks.smithy_rs, "/p2/smithy-rs");

        mocks
            .versions
            .expect_load()
            .withf(|p| p.to_string_lossy() == "/p2/aws-sdk-rust")
            .once()
            .returning(|_| {
                Ok(VersionsManifest {
                    smithy_rs_revision: "some-previous-commit-hash".into(),
                    aws_doc_sdk_examples_revision: "old-examples-hash".into(),
                })
            });
        mocks.set_smithyrs_commits_to_sync(
            "some-previous-commit-hash",
            &["hash-newest", "hash-oldest"],
        );
        set_head(&mut mocks.aws_doc_sdk_examples, "examples-head");

        expect_successful_smithyrs_sync(
            &mut mocks,
            &mut seq,
            Commit {
                hash: "hash-oldest".into(),
                author_name: "Some Dev".into(),
                author_email: "somedev@example.com".into(),
                message_subject: "Some commit subject".into(),
                message_body: "".into(),
            },
            "[smithy-rs] Some commit subject",
        );
        expect_successful_smithyrs_sync(
            &mut mocks,
            &mut seq,
            Commit {
                hash: "hash-newest".into(),
                author_name: "Another Dev".into(),
                author_email: "anotherdev@example.com".into(),
                message_subject: "Another commit subject".into(),
                message_body: "This one has a body\n\n- bullet\n- bullet\n\nmore".into(),
            },
            "[smithy-rs] Another commit subject\n\nThis one has a body\n\n- bullet\n- bullet\n\nmore",
        );

        expect_successful_example_sync(
            &mut mocks,
            &mut seq,
            "old-examples-hash",
            &[
                Commit {
                    hash: "hash2".into(),
                    author_name: "Some Example Writer".into(),
                    author_email: "someexamplewriter@example.com".into(),
                    message_subject: "More examples".into(),
                    message_body: "".into(),
                },
                Commit {
                    hash: "hash1".into(),
                    author_name: "Another Example Writer".into(),
                    author_email: "anotherexamplewriter@example.com".into(),
                    message_subject: "Another example".into(),
                    message_body: "This one has a body\n\n- bullet\n- bullet\n\nmore".into(),
                },
            ],
        );

        let sync = mocks.into_sync();
        sync.sync().expect("success");
    }

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
}
