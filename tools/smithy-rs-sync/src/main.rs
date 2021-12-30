/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

mod fs;

use crate::fs::{delete_all_generated_files_and_folders, find_handwritten_files_and_folders};
use anyhow::{anyhow, bail, Context, Result};
use git2::{Commit, IndexAddOption, ObjectType, Oid, Repository, ResetType, Signature};
use std::ffi::OsStr;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt(name = "smithy-rs-sync")]
/// A CLI tool to replay commits from smithy-rs, generate code, and commit that code to aws-rust-sdk.
struct Opt {
    /// The path to the smithy-rs repo folder
    #[structopt(long, parse(from_os_str))]
    smithy_rs: PathBuf,
    /// The path to the aws-sdk-rust folder
    #[structopt(long, parse(from_os_str))]
    aws_sdk: PathBuf,
    /// The branch in aws-sdk-rust that commits will be mirrored to
    #[structopt(long, default_value = "next")]
    branch: String,
}

const BOT_NAME: &str = "AWS SDK Rust Bot";
const BOT_EMAIL: &str = "aws-sdk-rust-primary@amazon.com";
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
    } = Opt::from_args();

    sync_aws_sdk_with_smithy_rs(&smithy_rs, &aws_sdk, &branch)
        .map_err(|e| e.context("The sync failed"))
}

/// Run through all commits made to `smithy-rs` since last sync and "replay" them onto `aws-sdk-rust`.
fn sync_aws_sdk_with_smithy_rs(smithy_rs: &Path, aws_sdk: &Path, branch: &str) -> Result<()> {
    // In case these are relative paths, canonicalize them into absolute paths
    let aws_sdk = aws_sdk.canonicalize().context(here!())?;
    let smithy_rs = smithy_rs.canonicalize().context(here!())?;

    eprintln!("aws-sdk-rust path:\t{}", aws_sdk.display());
    eprintln!("smithy-rs path:\t\t{}", smithy_rs.display());

    // Open the repositories we'll be working with
    let smithy_rs_repo = Repository::open(&smithy_rs).context("couldn't open smithy-rs repo")?;
    let aws_sdk_repo = Repository::open(&aws_sdk).context("couldn't open aws-sdk-rust repo")?;

    // Check repo that we're going to be moving the code into to see what commit it was last synced with
    let last_synced_commit =
        get_last_synced_commit(&aws_sdk).context("couldn't get last synced commit")?;
    let commit_revs = commits_to_be_applied(&smithy_rs_repo, &last_synced_commit)
        .context("couldn't build list of commits that need to be synced")?;

    if commit_revs.is_empty() {
        eprintln!("There are no new commits to be applied, have a nice day.");
        return Ok(());
    }

    // `git checkout` the branch of `aws-sdk-rust` that we want to replay commits onto.
    // By default, this is the `next` branch.
    checkout_branch_to_sync_to(&aws_sdk_repo, branch).with_context(|| {
        format!(
            "couldn't checkout {} branch of aws-sdk-rust for syncing",
            branch,
        )
    })?;

    let number_of_commits = commit_revs.len();
    println!(
        "Found {} unsynced commit(s), syncing now...",
        number_of_commits
    );
    // Run through all the new commits, syncing them one by one
    for (i, rev) in commit_revs.iter().enumerate() {
        let commit = smithy_rs_repo
            .find_commit(*rev)
            .with_context(|| format!("couldn't find commit {} in smithy-rs", rev))?;

        eprintln!("[{}/{}]\tsyncing {}...", i + 1, number_of_commits, rev);
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
        create_mirror_commit(&aws_sdk_repo, &commit)
            .context("couldn't commit SDK changes to aws-sdk-rust")?;
    }
    eprintln!("Successfully synced {} commit(s)", commit_revs.len());

    // Get the last commit we synced so that we can set that for the next time this tool gets run
    let last_synced_commit = commit_revs
        .last()
        .expect("can't be empty because we'd have early returned");
    eprintln!("Updating 'commit at last sync' to {}", last_synced_commit);

    // Update the file containing the commit hash
    set_last_synced_commit(&aws_sdk_repo, last_synced_commit).with_context(|| {
        format!(
            "couldn't write last synced commit hash ({}) to aws-sdk-rust/{}",
            last_synced_commit, COMMIT_HASH_FILENAME,
        )
    })?;

    eprintln!(
        "Successfully synced {} mirror commit(s) to aws-sdk-rust/{}. Don't forget to push them",
        number_of_commits, branch
    );

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
fn set_last_synced_commit(repo: &Repository, oid: &Oid) -> Result<()> {
    let repo_path = repo.workdir().expect("this will always exist");
    let path = repo_path.join(COMMIT_HASH_FILENAME);
    let mut file = OpenOptions::new().write(true).truncate(true).open(&path)?;

    file.write(oid.to_string().as_bytes())
        .with_context(|| format!("Couldn't write commit hash to '{}'", path.display()))?;

    Ok(())
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
    let _ = run(
        &[gradlew, "-Paws.fullsdk=true", ":aws:sdk:clean"],
        smithy_rs_path,
    )
    .context(here!())?;
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

    let from_path = from_path
        .to_str()
        .expect("for our use case, this will always be UTF-8");
    let to_path = to_path
        .to_str()
        .expect("for our use case, this will always be UTF-8");

    // This command uses absolute paths so working dir doesn't matter. Even so, we set
    // working dir to the dir this binary was run from because `run` expects one.
    let working_dir = std::env::current_dir().expect("can't access current working dir");
    let _ = run(&["cp", "-r", from_path, to_path], &working_dir).context(here!())?;

    eprintln!("\tsuccessfully copied built SDK");
    Ok(())
}

/// Find the last commit made to a repo.
fn find_last_commit(repo: &Repository) -> Result<Commit> {
    let obj = repo
        .head()
        .context(here!())?
        .resolve()
        .context(here!())?
        .peel(ObjectType::Commit)
        .context(here!())?;
    obj.into_commit()
        .map_err(|_| anyhow!("couldn't find last commit"))
}

/// Create a "mirror" commit. Works by reading a smithy-rs commit and then using the info
/// attached to it to create a commit in aws-sdk-rust. This also updates the `.smithyrs-githash`
/// file with the hash of `based_on_commit`.
fn create_mirror_commit(aws_sdk_repo: &Repository, based_on_commit: &Commit) -> Result<()> {
    eprintln!("\tcreating mirror commit...");

    // Update the file that tracks what smithy-rs commit the SDK was generated from
    set_last_synced_commit(aws_sdk_repo, &based_on_commit.id())?;

    let mut index = aws_sdk_repo.index().context(here!())?;
    // The equivalent of `git add .`
    index
        .add_all(["."].iter(), IndexAddOption::DEFAULT, None)
        .context(here!())?;
    let oid = index.write_tree().context(here!())?;
    let parent_commit = find_last_commit(aws_sdk_repo).context(here!())?;
    let tree = aws_sdk_repo.find_tree(oid).context(here!())?;
    let bot_signature = Signature::now(BOT_NAME, BOT_EMAIL).context(here!())?;

    let _ = aws_sdk_repo
        .commit(
            Some("HEAD"),
            &based_on_commit.author(),
            &bot_signature,
            based_on_commit.message().unwrap_or_default(),
            &tree,
            &[&parent_commit],
        )
        .context(here!())?;

    eprintln!("\tsuccessfully created mirror commit");

    Ok(())
}

/// `git checkout` the branch of aws-sdk-rust that we want to mirror commits to (defaults to `next`).
fn checkout_branch_to_sync_to(aws_sdk_repo: &Repository, aws_sdk_branch: &str) -> Result<()> {
    aws_sdk_repo
        .find_remote("origin")?
        .fetch(&["main"], None, None)?;
    let (object, _) = aws_sdk_repo.revparse_ext(aws_sdk_branch).context(here!())?;

    aws_sdk_repo.checkout_tree(&object, None).context(here!())?;

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

/// For a slice containing `S` where `S: AsRef<OsStr>`, join all `S` into a space-separated String.
fn stringify_args<S>(args: &[S]) -> String
where
    S: AsRef<OsStr>,
{
    let args: Vec<_> = args.iter().map(|s| s.as_ref().to_string_lossy()).collect();
    args.join(" ")
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
