/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use anyhow::Result;
use mockall::{predicate::*, Sequence};
use sdk_sync::git::{Commit, CommitHash};
use sdk_sync::sync::{Sync, BOT_EMAIL, BOT_NAME};
use sdk_sync::versions::VersionsManifest;
use std::path::{Path, PathBuf};

mockall::mock! {
    Fs {}
    impl sdk_sync::fs::Fs for Fs {
        fn delete_all_generated_files_and_folders(&self, directory: &Path) -> Result<()>;
        fn find_handwritten_files_and_folders(
            &self,
            aws_sdk_path: &Path,
            build_artifacts_path: &Path,
        ) -> Result<Vec<PathBuf>>;
        fn remove_dir_all_idempotent(&self, path: &Path) -> Result<()>;
        fn read_to_string(&self, path: &Path) -> Result<String>;
        fn remove_file(&self, path: &Path) -> Result<()>;
        fn recursive_copy(&self, source: &Path, destination: &Path) -> Result<()>;
    }
}

mockall::mock! {
    Git {}
    impl sdk_sync::git::Git for Git {
        fn path(&self) -> &Path;
        fn get_head_revision(&self) -> Result<CommitHash>;
        fn stage(&self, path: &Path) -> Result<()>;
        fn commit_on_behalf(
            &self,
            bot_name: &str,
            bot_email: &str,
            author_name: &str,
            author_email: &str,
            message: &str,
        ) -> Result<()>;
        fn commit(&self, name: &str, email: &str, message: &str) -> Result<()>;
        fn rev_list<'a>(
            &self,
            start_inclusive_revision: &str,
            end_exclusive_revision: &str,
            path: Option<&'a Path>,
        ) -> Result<Vec<CommitHash>>;
        fn show(&self, revision: &str) -> Result<Commit>;
        fn hard_reset(&self, revision: &str) -> Result<()>;
    }
}

mockall::mock! {
    Gradle {}
    impl sdk_sync::gradle::Gradle for Gradle {
        fn aws_sdk_clean(&self) -> Result<()>;
        fn aws_sdk_assemble(&self, examples_revision: &CommitHash) -> Result<()>;
    }
}

mockall::mock! {
    Versions {}
    impl sdk_sync::versions::Versions for Versions {
        fn load(&self, aws_sdk_rust_path: &Path) -> Result<VersionsManifest>;
    }
}

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
            .returning(|_, _, _| Ok(hashes.iter().map(|&hash| CommitHash::from(hash)).collect()));
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
            .withf(move |src, dst| src.to_string_lossy() == source && dst.to_string_lossy() == dest)
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
    mocks
        .set_smithyrs_commits_to_sync("some-previous-commit-hash", &["hash-newest", "hash-oldest"]);
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
