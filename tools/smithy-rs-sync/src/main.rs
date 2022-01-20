/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

mod fs;

use crate::fs::{delete_all_generated_files_and_folders, find_handwritten_files_and_folders};
use anyhow::{anyhow, bail, Context, Result};
use git2::{Commit, Oid, Repository, ResetType};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt(name = "smithy-rs-sync")]
/// A CLI tool to replay commits from smithy-rs, generate code, and commit that code to aws-rust-sdk.
struct Opt {
    /// The path to the smithy-rs repo folder.
    #[structopt(long, parse(from_os_str))]
    smithy_rs: PathBuf,
    /// The path to the aws-sdk-rust folder.
    #[structopt(long, parse(from_os_str))]
    aws_sdk: PathBuf,
    /// The branch in aws-sdk-rust that commits will be mirrored to.
    #[structopt(long, default_value = "next")]
    branch: String,
    /// The maximum amount of commits to sync in one run.
    #[structopt(long, default_value = "5")]
    max_commits_to_sync: usize,
}

const BOT_NAME: &str = "AWS SDK Rust Bot";
const BOT_EMAIL: &str = "aws-sdk-rust-primary@amazon.com";
const BOT_COMMIT_PREFIX: &str = "[autosync]";
const COMMIT_HASH_FILENAME: &str = ".smithyrs-githash";

/// A macro for attaching info to error messages pointing to the line of code responsible for the error.
/// [Thanks to dtolnay for this macro](https://github.com/dtolnay/anyhow/issues/22#issuecomment-542309452)
macro_rules! here {
    () => {
        concat!("error at ", file!(), ":", line!(), ":", column!())
    };
}

// export this macro for use in other modules in this crate
pub(crate) use here;

/// Run this app in order to keep aws-sdk-rust in sync with smithy-rs.
///
/// pre-requisites:
/// - an up-to-date local copy of smithy-rs repo
/// - an up-to-date local copy of aws-sdk-rs repo
/// - a Unix-ey system (for the `cp` and `rf` commands to work)
/// - Java Runtime Environment v11 (in order to run gradle commands)
///
/// ```sh
/// cargo run -- \
/// --smithy-rs /Users/zhessler/Documents/smithy-rs-test/ \
/// --aws-sdk /Users/zhessler/Documents/aws-sdk-rust-test/
/// ```
fn main() -> Result<()> {
    let Opt {
        smithy_rs,
        aws_sdk,
        branch,
        max_commits_to_sync,
    } = Opt::from_args();

    sync_aws_sdk_with_smithy_rs(&smithy_rs, &aws_sdk, &branch, max_commits_to_sync)
        .map_err(|e| e.context("The sync failed"))
}

/// Run through all commits made to `smithy-rs` since last sync and "replay" them onto `aws-sdk-rust`.
fn sync_aws_sdk_with_smithy_rs(
    smithy_rs: &Path,
    aws_sdk: &Path,
    branch: &str,
    max_commits_to_sync: usize,
) -> Result<()> {
    // In case these are relative paths, canonicalize them into absolute paths
    let aws_sdk = aws_sdk.canonicalize().context(here!())?;
    let smithy_rs = smithy_rs.canonicalize().context(here!())?;

    eprintln!("aws-sdk-rust path:\t{}", aws_sdk.display());
    if !is_a_git_repository(&aws_sdk) {
        eprintln!("warning: aws-sdk-rust is not a git repository");
    }

    eprintln!("smithy-rs path:\t\t{}", smithy_rs.display());
    if !is_a_git_repository(&aws_sdk) {
        eprintln!("warning: smithy-rs is not a git repository");
    }

    // Open the repositories we'll be working with
    let smithy_rs_repo = Repository::open(&smithy_rs).context("couldn't open smithy-rs repo")?;

    // Check repo that we're going to be moving the code into to see what commit it was last synced with
    let last_synced_commit =
        get_last_synced_commit(&aws_sdk).context("couldn't get last synced commit")?;
    let commit_revs = commits_to_be_applied(&smithy_rs_repo, &last_synced_commit)
        .context("couldn't build list of commits that need to be synced")?;

    if commit_revs.is_empty() {
        eprintln!("There are no new commits to be applied, have a nice day.");
        return Ok(());
    }

    let total_number_of_commits = commit_revs.len();
    let number_of_commits_to_sync = max_commits_to_sync.min(total_number_of_commits);
    eprintln!(
        "Syncing {} of {} un-synced commit(s)...",
        number_of_commits_to_sync, total_number_of_commits
    );

    // Run through all the new commits, syncing them one by one
    for (i, rev) in commit_revs.iter().enumerate().take(max_commits_to_sync) {
        let commit = smithy_rs_repo
            .find_commit(*rev)
            .with_context(|| format!("couldn't find commit {} in smithy-rs", rev))?;

        eprintln!(
            "[{}/{}]\tsyncing {}...",
            i + 1,
            number_of_commits_to_sync,
            rev
        );
        checkout_commit_to_sync_from(&smithy_rs_repo, &commit).with_context(|| {
            format!(
                "couldn't checkout commit {} from smithy-rs that we needed for syncing",
                rev,
            )
        })?;

        let build_artifacts = build_sdk(&smithy_rs).context("couldn't build SDK")?;
        clean_out_existing_sdk(&aws_sdk)
            .context("couldn't clean out existing SDK from aws-sdk-rust")?;

        // Check that we aren't generating any files that we've marked as "handwritten"
        let handwritten_files_in_generated_sdk_folder =
            find_handwritten_files_and_folders(&aws_sdk, &build_artifacts)?;
        if !handwritten_files_in_generated_sdk_folder.is_empty() {
            bail!(
                "found one or more 'handwritten' files/folders in generated code: {:#?}\nhint: if this file is newly generated, remove it from .handwritten",
                handwritten_files_in_generated_sdk_folder
            );
        }

        copy_sdk(&build_artifacts, &aws_sdk)?;
        create_mirror_commit(&aws_sdk, &commit)
            .context("couldn't commit SDK changes to aws-sdk-rust")?;
    }

    eprintln!(
        "Successfully synced {} mirror commit(s) to aws-sdk-rust/{}. Don't forget to push them",
        number_of_commits_to_sync, branch
    );

    let number_of_un_synced_commits =
        total_number_of_commits.saturating_sub(number_of_commits_to_sync);
    if number_of_un_synced_commits != 0 {
        eprintln!(
            "At least {} commits still need to be synced, please run the tool again",
            number_of_un_synced_commits
        );
    }

    Ok(())
}

/// Starting from a given commit, walk the tree to its `HEAD` in order to build a list of commits that we'll
/// need to sync. If you don't see the commits you're expecting, make sure the repo is up to date.
/// This function doesn't include the `since_commit` in the list since that commit was synced last time
/// this tool was run.
fn commits_to_be_applied(smithy_rs_repo: &Repository, since_commit: &Oid) -> Result<Vec<Oid>> {
    let rev_range = format!("{}..HEAD", since_commit);
    eprintln!("Checking for smithy-rs commits in range {}", rev_range);

    let mut rev_walk = smithy_rs_repo.revwalk().context(here!())?;
    rev_walk.push_range(&rev_range).context(here!())?;

    let mut commit_revs = rev_walk
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .context(here!())?;

    // Order the revs from earliest to latest
    commit_revs.reverse();

    Ok(commit_revs)
}

/// Read the file from aws-sdk-rust that tracks the last smithy-rs commit it was synced with.
/// Returns the hash of that commit.
fn get_last_synced_commit(repo_path: &Path) -> Result<Oid> {
    let path = repo_path.join(COMMIT_HASH_FILENAME);

    let commit_hash = std::fs::read_to_string(&path)
        .with_context(|| format!("couldn't get commit hash from file at '{}'", path.display()))?;

    // We trim here in case some really helpful IDE added a newline to the file
    let oid = Oid::from_str(commit_hash.trim()).context(here!())?;

    Ok(oid)
}

/// Write the last synced commit to the file in aws-sdk-rust that tracks the last smithy-rs commit it was synced with.
fn set_last_synced_commit(repo_path: &Path, oid: &Oid) -> Result<()> {
    let oid_string = oid.to_string();
    let oid_bytes = oid_string.as_bytes();
    let path = repo_path.join(COMMIT_HASH_FILENAME);

    std::fs::write(&path, oid_bytes)
        .with_context(|| format!("Couldn't write commit hash to '{}'", path.display()))
}

/// Run the necessary commands to build the SDK. On success, returns the path to the folder containing
/// the build artifacts.
fn build_sdk(smithy_rs_path: &Path) -> Result<PathBuf> {
    eprintln!("\tbuilding the SDK...");
    let start = Instant::now();
    let gradlew = smithy_rs_path.join("gradlew");
    let gradlew = gradlew
        .to_str()
        .expect("for our use case, this will always be UTF-8");

    // The output of running these commands isn't logged anywhere unless they fail
    let _ = run(&["rm", "-rf", "aws/sdk/build"], smithy_rs_path).context(here!())?;
    let _ = run(&[gradlew, ":aws:sdk:clean"], smithy_rs_path).context(here!())?;
    let _ = run(
        &[gradlew, "-Paws.fullsdk=true", ":aws:sdk:assemble"],
        smithy_rs_path,
    )
    .context(here!())?;

    let build_artifact_path = smithy_rs_path.join("aws/sdk/build/aws-sdk");
    eprintln!("\tsuccessfully built the SDK in {:?}", start.elapsed());
    Ok(build_artifact_path)
}

/// Delete any current SDK files in aws-sdk-rust. Run this before copying over new files.
fn clean_out_existing_sdk(aws_sdk_path: &Path) -> Result<()> {
    eprintln!("\tcleaning out previously built SDK...");
    let start = Instant::now();

    delete_all_generated_files_and_folders(aws_sdk_path).context(here!())?;

    eprintln!(
        "\tsuccessfully cleaned out previously built SDK in {:?}",
        start.elapsed()
    );
    Ok(())
}

/// Use `cp -r` to recursively copy all files and folders from the smithy-rs build artifacts folder
/// to the aws-sdk-rust repo folder. Paths passed in must be absolute.
fn copy_sdk(from_path: &Path, to_path: &Path) -> Result<()> {
    eprintln!("\tcopying built SDK...");

    if !from_path.is_absolute() {
        bail!(
            "expected absolute from_path but got: {}",
            from_path.display()
        );
    } else if !to_path.is_absolute() {
        bail!("expected absolute to_path but got: {}", from_path.display());
    }

    // The '.' tells cp to copy the folder contents, not the folder
    let from_path = from_path.join(".");
    let from_path = from_path
        .to_str()
        .expect("for our use case, this will always be UTF-8");
    let to_path = to_path
        .to_str()
        .expect("for our use case, this will always be UTF-8");

    // This command uses absolute paths so working dir doesn't matter. Even so, we set
    // working dir to the dir this binary was run from because `run` expects one.
    // GitHub actions don't support current_dir so we use current_exe
    let exe_dir = std::env::current_exe().expect("can't access path of this exe");
    let working_dir = exe_dir.parent().expect("exe is not in a folder?");

    let _ = run(&["cp", "-r", from_path, to_path], working_dir).context(here!())?;

    eprintln!("\tsuccessfully copied built SDK");
    Ok(())
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
fn create_mirror_commit(aws_sdk_path: &Path, based_on_commit: &Commit) -> Result<()> {
    eprintln!("\tcreating mirror commit...");

    // Update the file that tracks what smithy-rs commit the SDK was generated from
    set_last_synced_commit(aws_sdk_path, &based_on_commit.id()).context(here!())?;

    run(&["git", "add", "."], aws_sdk_path).context(here!())?;
    run(
        &[
            "git",
            // Set the committer of this commit
            "-c",
            &format!("user.name={}", BOT_NAME),
            "-c",
            &format!("user.email={}", BOT_EMAIL),
            "commit",
            // Set the commit message
            "-m",
            // Even thought we're inserting user input here, we're safe from shell injection because
            // the Rust Command API passes args directly to the application to be run
            &format!(
                "{} {}",
                BOT_COMMIT_PREFIX,
                based_on_commit.message().unwrap_or_default()
            ),
            // Set the original author of this commit
            "--author",
            &based_on_commit.author().to_string(),
        ],
        aws_sdk_path,
    )
    .context(here!())?;
    let commit_hash = find_last_commit(aws_sdk_path).context(here!())?;

    eprintln!("\tsuccessfully created mirror commit {}", commit_hash);

    Ok(())
}

/// `git checkout` a commit from smithy-rs that we're going to mirror to aws-sdk-rust.
fn checkout_commit_to_sync_from(smithy_rs_repo: &Repository, commit: &Commit) -> Result<()> {
    let head = smithy_rs_repo
        .head()
        .context(here!())?
        .target()
        .context(here!())?;
    let head = smithy_rs_repo.find_object(head, None).context(here!())?;
    smithy_rs_repo
        .reset(&head, ResetType::Hard, None)
        .context(here!())?;

    smithy_rs_repo
        .checkout_tree(commit.as_object(), None)
        .context(here!())?;

    Ok(())
}

/// Run a shell command from a given working directory.
fn run<S>(args: &[S], working_dir: &Path) -> Result<()>
where
    S: AsRef<OsStr>,
{
    if args.is_empty() {
        bail!("args slice passed to run must have length >= 1");
    }

    let command_output = Command::new(&args[0])
        .args(&args[1..])
        .current_dir(working_dir)
        .output()
        .with_context(|| {
            format!(
                "failed to execute '{}' in dir '{}'",
                stringify_args(args),
                working_dir.display()
            )
        })?;

    if !command_output.status.success() {
        let stderr = String::from_utf8_lossy(&command_output.stderr);
        let stdout = String::from_utf8_lossy(&command_output.stdout);

        eprintln!("stdout:\n{}\n", stdout);
        eprintln!("stderr:\n{}\n", stderr);

        bail!(
            "command '{}' exited with a non-zero status",
            stringify_args(args)
        )
    }

    Ok(())
}

fn find_last_commit(repo_path: &Path) -> Result<String> {
    let output = Command::new("git")
        .arg("rev-parse")
        .arg("HEAD")
        .current_dir(&repo_path)
        .output()
        .map_err(|err| anyhow!("couldn't get commit hash: {}", err))?;

    let hash = String::from_utf8_lossy(&output.stdout);

    Ok(hash.to_string())
}

/// For a slice containing `S` where `S: AsRef<OsStr>`, join all `S` into a space-separated String.
fn stringify_args<S>(args: &[S]) -> String
where
    S: AsRef<OsStr>,
{
    let args: Vec<_> = args.iter().map(|s| s.as_ref().to_string_lossy()).collect();
    args.join(" ")
}

fn _is_running_in_github_action() -> bool {
    std::env::var("GITHUB_ACTIONS").unwrap_or_default() == "true"
}

fn is_a_git_repository(dir: &Path) -> bool {
    dir.join(".git").is_dir()
}

#[cfg(test)]
mod tests {
    use super::stringify_args;

    #[test]
    fn test_stringify_args() {
        let args = &["this", "is", "a", "test"];
        let expected = "this is a test";
        let actual = stringify_args(args);

        assert_eq!(expected, actual);
    }
}
