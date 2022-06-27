/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use self::gen::{CodeGenSettings, DefaultSdkGenerator, SdkGenerator};
use crate::fs::{DefaultFs, Fs};
use crate::git::{Commit, Git, GitCLI};
use crate::versions::{DefaultVersions, Versions, VersionsManifest};
use anyhow::{Context, Result};
use smithy_rs_tool_common::macros::here;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Sender, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use systemstat::{ByteSize, Platform, System};
use tracing::{debug, info, info_span, warn};
use tracing_attributes::instrument;

pub mod gen;

pub const BOT_NAME: &str = "AWS SDK Rust Bot";
pub const BOT_EMAIL: &str = "aws-sdk-rust-primary@amazon.com";
pub const MODEL_STASH_BRANCH_NAME: &str = "__sdk_sync__models_";

#[derive(Default)]
struct SyncProgress {
    commits_completed: AtomicUsize,
    total_commits: AtomicUsize,
}

struct ProgressThread {
    handle: Option<thread::JoinHandle<()>>,
    tx: Sender<bool>,
}

impl ProgressThread {
    pub fn spawn(progress: Arc<SyncProgress>) -> ProgressThread {
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = thread::spawn(move || {
            let mut done = false;
            let system = System::new();
            while !done {
                let cpu = system.cpu_load_aggregate().ok();
                for _ in 0..15 {
                    thread::sleep(Duration::from_secs(1));
                    if !matches!(rx.try_recv(), Err(TryRecvError::Empty)) {
                        done = true;
                        break;
                    }
                }
                let cpu = if let Some(Ok(cpu)) = cpu.map(|cpu| cpu.done()) {
                    format!("{:.1}", 100.0 - cpu.idle * 100.0)
                } else {
                    "error".to_string()
                };
                let (memory, swap) = system.memory_and_swap().unwrap();
                info!(
                    "Progress: smithy-rs commit {}/{}, cpu use: {}, memory used: {}, swap used: {}",
                    progress.commits_completed.load(Ordering::Relaxed),
                    progress.total_commits.load(Ordering::Relaxed),
                    cpu,
                    Self::format_memory(memory.free, memory.total),
                    Self::format_memory(swap.free, swap.total),
                );
            }
        });
        ProgressThread {
            handle: Some(handle),
            tx,
        }
    }

    fn format_memory(free: ByteSize, total: ByteSize) -> String {
        let (free, total) = (free.as_u64(), total.as_u64());
        let format_part = |val: u64| format!("{:.3}GB", val as f64 / 1024.0 / 1024.0 / 1024.0);
        format!(
            "{}/{}",
            format_part(total.saturating_sub(free)),
            format_part(total)
        )
    }
}

impl Drop for ProgressThread {
    fn drop(&mut self) {
        // Attempt to stop the loop in the thread
        let _ = self.tx.send(true);
        let _ = self.handle.take().map(|handle| handle.join());
    }
}

pub struct Sync {
    aws_doc_sdk_examples: Arc<dyn Git>,
    aws_sdk_rust: Arc<dyn Git>,
    smithy_rs: Arc<dyn Git>,
    fs: Arc<dyn Fs>,
    versions: Arc<dyn Versions>,
    previous_versions_manifest: Arc<PathBuf>,
    codegen_settings: CodeGenSettings,
    progress: Arc<SyncProgress>,
    // Keep a reference to the temp directory so that it doesn't get cleaned up until the sync is complete
    _temp_dir: Arc<tempfile::TempDir>,
}

impl Sync {
    pub fn new(
        aws_doc_sdk_examples_path: &Path,
        aws_sdk_rust_path: &Path,
        smithy_rs_path: &Path,
        codegen_settings: CodeGenSettings,
    ) -> Result<Self> {
        let _temp_dir = Arc::new(tempfile::tempdir().context(here!("create temp dir"))?);
        let aws_sdk_rust = Arc::new(GitCLI::new(aws_sdk_rust_path)?);
        let fs = Arc::new(DefaultFs::new()) as Arc<dyn Fs>;
        let previous_versions_manifest =
            Arc::new(_temp_dir.path().join("previous-release-versions.toml"));
        fs.copy(
            &aws_sdk_rust.path().join("versions.toml"),
            &previous_versions_manifest,
        )
        .context("failed to copy versions.toml to temp dir")?;

        Ok(Self {
            aws_doc_sdk_examples: Arc::new(GitCLI::new(aws_doc_sdk_examples_path)?),
            aws_sdk_rust,
            smithy_rs: Arc::new(GitCLI::new(smithy_rs_path)?),
            fs,
            versions: Arc::new(DefaultVersions::new()),
            previous_versions_manifest,
            codegen_settings,
            progress: Default::default(),
            _temp_dir,
        })
    }

    #[cfg(test)]
    pub fn new_with(
        aws_doc_sdk_examples: impl Git + 'static,
        aws_sdk_rust: impl Git + 'static,
        smithy_rs: impl Git + 'static,
        fs: impl Fs + 'static,
        versions: impl Versions + 'static,
    ) -> Self {
        Self {
            aws_doc_sdk_examples: Arc::new(aws_doc_sdk_examples),
            aws_sdk_rust: Arc::new(aws_sdk_rust),
            smithy_rs: Arc::new(smithy_rs),
            fs: Arc::new(fs),
            versions: Arc::new(versions),
            previous_versions_manifest: Arc::new(PathBuf::from("doesnt-matter-for-tests")),
            codegen_settings: Default::default(),
            progress: Default::default(),
            _temp_dir: Arc::new(tempfile::tempdir().unwrap()),
        }
    }

    #[instrument(skip(self))]
    pub fn sync(&self) -> Result<()> {
        let _progress_thread = ProgressThread::spawn(self.progress.clone());

        info!("Loading versions.toml...");
        let versions = self
            .versions
            .load(self.aws_sdk_rust.path())
            .context("load versions.toml")?;
        info!("{:?}", versions);

        let has_model_changes = self.stash_model_changes().context("stash model changes")?;
        self.sync_smithy_rs(&versions).context("sync smithy-rs")?;
        if has_model_changes {
            self.sync_model_changes(&versions)
                .context("sync model changes")?;
        }
        self.sync_examples(&versions).context("sync examples")?;

        Ok(())
    }

    /// Stores model changes in another branch so that the smithy-rs sync and example sync
    /// can be done with the old models to keep the changes isolated to their individual commits.
    /// Returns `true` if there are model changes.
    #[instrument(skip(self))]
    fn stash_model_changes(&self) -> Result<bool> {
        info!("Stashing model changes...");
        let original_revision = self.smithy_rs.get_head_revision().context(here!())?;
        info!(
            "smithy-rs revision with model changes: {}",
            original_revision
        );

        // Create a branch to hold the model changes without switching to it
        self.smithy_rs
            .create_branch(MODEL_STASH_BRANCH_NAME, "HEAD")
            .context(here!())?;
        // Get the name of the current branch
        let branch_name = self.smithy_rs.current_branch_name().context(here!())?;
        // Reset the start branch to what's in origin
        self.smithy_rs
            .hard_reset(&format!("origin/{}", branch_name))
            .context(here!())?;

        let head = self.smithy_rs.get_head_revision().context(here!())?;
        info!("smithy-rs revision without model changes: {}", head);
        let has_model_changes = head != original_revision;
        info!("smithy-rs has model changes: {}", has_model_changes);
        Ok(has_model_changes)
    }

    /// Restore the model changes and generate code based on them.
    #[instrument(skip(self))]
    fn sync_model_changes(&self, versions: &VersionsManifest) -> Result<()> {
        info!("Syncing model changes...");

        // Restore the model changes. Note: endpoints.json/default config/model changes
        // may each be in their own commits coming into this, but we want them squashed into
        // one commit for smithy-rs.
        self.smithy_rs
            .squash_merge(
                BOT_NAME,
                BOT_EMAIL,
                MODEL_STASH_BRANCH_NAME,
                "Update SDK models",
            )
            .context(here!())?;
        self.smithy_rs
            .delete_branch(MODEL_STASH_BRANCH_NAME)
            .context(here!())?;
        let model_change_commit = self.smithy_rs.show("HEAD").context(here!())?;

        // Generate with the original examples
        let sdk_gen = DefaultSdkGenerator::new(
            &self.previous_versions_manifest,
            &versions.aws_doc_sdk_examples_revision,
            &self.aws_sdk_rust.path().join("examples"),
            self.fs.clone(),
            None,
            self.smithy_rs.path(),
            &self.codegen_settings,
        )
        .context(here!())?;
        let generated_sdk = sdk_gen.generate_sdk().context(here!())?;
        self.copy_sdk(generated_sdk.path())
            .context(here!("failed to copy the SDK"))?;

        // Commit changes if there are any
        if self.sdk_has_changes().context(here!())? {
            self.aws_sdk_rust
                .stage(&PathBuf::from("."))
                .context(here!())?;
            self.aws_sdk_rust
                .commit(BOT_NAME, BOT_EMAIL, &model_change_commit.message())
                .context(here!())?;
        }

        Ok(())
    }

    /// Run through all commits made to `smithy-rs` since last sync and "replay" them onto `aws-sdk-rust`.
    #[instrument(skip(self, versions))]
    fn sync_smithy_rs(&self, versions: &VersionsManifest) -> Result<()> {
        use rayon::prelude::*;

        info!(
            "Checking for smithy-rs commits in range HEAD..{}",
            versions.smithy_rs_revision
        );
        let mut commits = self
            .smithy_rs
            .rev_list("HEAD", versions.smithy_rs_revision.as_ref(), None)
            .context(here!())?;
        commits.reverse(); // Order the revs from earliest to latest

        if commits.is_empty() {
            info!("There are no new commits to be applied, have a nice day.");
            return Ok(());
        }

        info!("Syncing {} commit(s)...", commits.len());
        self.progress
            .total_commits
            .store(commits.len(), Ordering::Relaxed);

        // Generate code in parallel for each individual commit
        let code_gen_paths = {
            let previous_versions_manifest = self.previous_versions_manifest.clone();
            let smithy_rs = self.smithy_rs.clone();
            let examples_revision = versions.aws_doc_sdk_examples_revision.clone();
            let examples_path = self.aws_sdk_rust.path().join("examples");
            let fs = self.fs.clone();
            let codegen_settings = self.codegen_settings.clone();
            let progress = self.progress.clone();

            commits
                .par_iter()
                .enumerate()
                .map(move |(commit_num, commit_hash)| {
                    let span = info_span!(
                        "codegen",
                        commit_num = commit_num,
                        commit_hash = commit_hash.as_ref()
                    );
                    let _enter = span.enter();

                    let commit = smithy_rs.show(commit_hash.as_ref()).with_context(|| {
                        format!("couldn't find commit {} in smithy-rs", commit_hash)
                    })?;

                    let sdk_gen = DefaultSdkGenerator::new(
                        &previous_versions_manifest,
                        &examples_revision,
                        &examples_path,
                        fs.clone(),
                        Some(commit.hash.clone()),
                        smithy_rs.path(),
                        &codegen_settings,
                    )
                    .context(here!())?;
                    let sdk_path = sdk_gen.generate_sdk().context(here!())?;
                    progress.commits_completed.fetch_add(1, Ordering::Relaxed);
                    Ok((commit, sdk_path))
                })
                .collect::<Result<Vec<_>>>()?
        };

        for (commit, generated_sdk) in code_gen_paths {
            self.copy_sdk(generated_sdk.path())
                .context("failed to copy the SDK")?;
            self.commit_sdk_changes("[smithy-rs]", &commit)
                .context("couldn't commit SDK changes to aws-sdk-rust")?;
        }

        info!(
            "Successfully synced {} smithy-rs commit(s) to aws-sdk-rust",
            commits.len(),
        );

        Ok(())
    }

    /// Aggregate the commits made to Rust SDK examples into one while maintaining attribution.
    /// We squash all the example commits since the examples repo does merges rather than squash merges,
    /// and we prefer squash merges in aws-sdk-rust.
    #[instrument(skip(self, versions))]
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
            info!("No example changes to copy over.");
            return Ok(());
        }
        let examples_head = example_revisions.iter().cloned().next().unwrap();

        let sdk_gen = DefaultSdkGenerator::new(
            &self.previous_versions_manifest,
            &examples_head,
            &self.aws_doc_sdk_examples.path().join("rust_dev_preview"),
            self.fs.clone(),
            None,
            self.smithy_rs.path(),
            &self.codegen_settings,
        )
        .context(here!())?;
        let generated_sdk = sdk_gen.generate_sdk().context(here!())?;
        self.copy_sdk(generated_sdk.path())
            .context("failed to copy the SDK")?;
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
    #[instrument(skip(self))]
    fn copy_sdk(&self, generated_sdk_path: &Path) -> Result<()> {
        self.clean_out_existing_sdk()
            .context("couldn't clean out existing SDK from aws-sdk-rust")?;

        // Check that we aren't generating any files that we've marked as "handwritten"
        let handwritten_files_in_generated_sdk_folder = self
            .fs
            .find_handwritten_files_and_folders(self.aws_sdk_rust.path(), generated_sdk_path)
            .context(here!())?;
        if !handwritten_files_in_generated_sdk_folder.is_empty() {
            // TODO(https://github.com/awslabs/smithy-rs/issues/1493): This can be changed back to `bail!` after release decoupling is completed
            warn!(
                "found one or more 'handwritten' files/folders in generated code: {:#?}\nhint: if this file is newly generated, remove it from .handwritten",
                handwritten_files_in_generated_sdk_folder
            );
        }

        self.copy_sdk_files(generated_sdk_path, self.aws_sdk_rust.path())?;
        Ok(())
    }

    /// Recursively copy all files and folders from the smithy-rs build artifacts folder
    /// to the aws-sdk-rust repo folder. Paths passed in must be absolute.
    #[instrument(skip(self))]
    fn copy_sdk_files(&self, from_path: &Path, to_path: &Path) -> Result<()> {
        info!("Copying generated SDK...");

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

        info!("Successfully copied generated SDK");
        Ok(())
    }

    /// Returns true if the aws-sdk-rust repo has changes (excluding changes to versions.toml).
    /// The versions.toml shouldn't be considered since there's a high probability it is only
    /// a `smithy_rs_revision` change. It can also safely be ignored since any changes to version
    /// numbers will show up in individual crate manifests.
    #[instrument(skip(self))]
    fn sdk_has_changes(&self) -> Result<bool> {
        let untracked_files = self.aws_sdk_rust.untracked_files()?;
        let changed_files = self.aws_sdk_rust.changed_files()?;
        let has_changes = !untracked_files.is_empty()
            || !changed_files.is_empty()
                && (changed_files.len() != 1 || changed_files[0].to_str() != Some("versions.toml"));
        debug!("aws-sdk-rust untracked files: {:?}", untracked_files);
        debug!("aws-sdk-rust changed files: {:?}", changed_files);
        info!(
            "aws-sdk-rust has changes (not considering versions.toml): {}",
            has_changes
        );
        Ok(has_changes)
    }

    /// Commit the changes to aws-sdk-rust reflecting the info from a commit in another repository.
    #[instrument(skip(self))]
    fn commit_sdk_changes(&self, prefix: &str, based_on_commit: &Commit) -> Result<()> {
        if self.sdk_has_changes()? {
            info!("Committing generated SDK...");

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
            info!("Successfully committed {}", commit_hash);
        } else {
            info!(
                "No changes to the SDK found for commit: {}. Skipping.",
                based_on_commit.hash
            );
        }
        Ok(())
    }

    /// Delete any current SDK files in aws-sdk-rust. Run this before copying over new files.
    #[instrument(skip(self))]
    fn clean_out_existing_sdk(&self) -> Result<()> {
        info!("Cleaning out previously generated SDK...");
        self.fs
            .delete_all_generated_files_and_folders(self.aws_sdk_rust.path())
            .context(here!())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::MockFs;
    use crate::git::{CommitHash, MockGit};
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

    fn expect_has_changes(repo: &mut MockGit, changes: bool) {
        repo.expect_untracked_files()
            .once()
            .returning(move || Ok(Vec::new()));
        repo.expect_changed_files().once().returning(move || {
            Ok(if changes {
                vec![PathBuf::from("some-file")]
            } else {
                Vec::new()
            })
        });
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
        expect_has_changes(&mut aws_sdk_rust, false);

        // No staging or committing should occur
        aws_sdk_rust.expect_stage().never();
        aws_sdk_rust.expect_commit_on_behalf().never();

        let sync = Sync::new_with(
            MockGit::new(),
            aws_sdk_rust,
            MockGit::new(),
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
        expect_has_changes(&mut aws_sdk_rust, true);

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

    #[test]
    fn test_sdk_has_changes_no_changes_at_all() {
        let mut aws_sdk_rust = MockGit::new();
        aws_sdk_rust
            .expect_untracked_files()
            .once()
            .returning(move || Ok(Vec::new()));
        aws_sdk_rust
            .expect_changed_files()
            .once()
            .returning(move || Ok(Vec::new()));

        let sync = Sync::new_with(
            MockGit::new(),
            aws_sdk_rust,
            MockGit::new(),
            MockFs::new(),
            MockVersions::new(),
        );

        assert!(
            !sync.sdk_has_changes().unwrap(),
            "it should not have changes"
        );
    }

    #[test]
    fn test_sdk_has_changes_only_versions_toml_changed() {
        let mut aws_sdk_rust = MockGit::new();
        aws_sdk_rust
            .expect_untracked_files()
            .once()
            .returning(move || Ok(Vec::new()));
        aws_sdk_rust
            .expect_changed_files()
            .once()
            .returning(move || Ok(vec![PathBuf::from("versions.toml")]));

        let sync = Sync::new_with(
            MockGit::new(),
            aws_sdk_rust,
            MockGit::new(),
            MockFs::new(),
            MockVersions::new(),
        );

        assert!(
            !sync.sdk_has_changes().unwrap(),
            "it should not have changes"
        );
    }

    #[test]
    fn test_sdk_has_changes_untracked_files() {
        let mut aws_sdk_rust = MockGit::new();
        aws_sdk_rust
            .expect_untracked_files()
            .once()
            .returning(move || Ok(vec![PathBuf::from("some-new-file")]));
        aws_sdk_rust
            .expect_changed_files()
            .once()
            .returning(move || Ok(vec![PathBuf::from("versions.toml")]));

        let sync = Sync::new_with(
            MockGit::new(),
            aws_sdk_rust,
            MockGit::new(),
            MockFs::new(),
            MockVersions::new(),
        );

        assert!(sync.sdk_has_changes().unwrap(), "it should have changes");
    }

    #[test]
    fn test_sdk_has_changes_changed_files() {
        let mut aws_sdk_rust = MockGit::new();
        aws_sdk_rust
            .expect_untracked_files()
            .once()
            .returning(move || Ok(Vec::new()));
        aws_sdk_rust
            .expect_changed_files()
            .once()
            .returning(move || {
                Ok(vec![
                    PathBuf::from("versions.toml"),
                    PathBuf::from("something-else"),
                ])
            });

        let sync = Sync::new_with(
            MockGit::new(),
            aws_sdk_rust,
            MockGit::new(),
            MockFs::new(),
            MockVersions::new(),
        );

        assert!(sync.sdk_has_changes().unwrap(), "it should have changes");
    }
}
