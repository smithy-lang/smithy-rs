/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use anyhow::{anyhow, bail, Context, Result};
use git2::{Commit, IndexAddOption, ObjectType, Oid, Repository, ResetType, Signature};
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt(name = "smithy-rs-sync")]
/// A CLI tool to replay commits from smithy-rs, generate code, and commit that code to aws-rust-sdk
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

/// A macro for attaching info to error messages pointing to the line of code responsible for the error
/// [Thanks to dtolnay for this macro](https://github.com/dtolnay/anyhow/issues/22#issuecomment-542309452)
macro_rules! here {
    () => {
        concat!("error at ", file!(), ":", line!(), ":", column!())
    };
}

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
fn main() {
    let Opt {
        smithy_rs,
        aws_sdk,
        branch,
    } = Opt::from_args();

    if let Err(e) = sync_aws_sdk_with_smithy_rs(&smithy_rs, &aws_sdk, &branch) {
        eprintln!("Sync failed with error: {:?}", e);
    };
}

/// Run through all commits made to `smithy-rs` since last sync and "replay" them onto `aws-sdk-rust`
fn sync_aws_sdk_with_smithy_rs(smithy_rs: &Path, aws_sdk: &Path, branch: &str) -> Result<()> {
    // Open the repositories we'll be working with
    let smithy_rs_repo = Repository::open(smithy_rs).context("couldn't open smithy-rs repo")?;
    let aws_sdk_repo = Repository::open(aws_sdk).context("couldn't open aws-sdk-rust repo")?;

    // Check repo that we're going to be moving the code into to see what commit it was last synced with
    let last_synced_commit =
        get_last_synced_commit(aws_sdk).context("couldn't get last synced commit")?;
    let commit_revs = commits_to_be_applied(&smithy_rs_repo, &last_synced_commit)
        .context("couldn't build list of commits that need to be synced")?;

    if commit_revs.is_empty() {
        println!("There are no new commits to be applied, have a nice day.");
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

        println!("[{}/{}]\tsyncing {}...", i + 1, number_of_commits, rev);
        checkout_commit_to_sync_from(&smithy_rs_repo, &commit).with_context(|| {
            format!(
                "couldn't checkout commit {} from smithy-rs that we needed for syncing",
                rev,
            )
        })?;

        let build_artifacts = build_sdk(smithy_rs).context("couldn't build SDK")?;
        clean_out_existing_sdk(aws_sdk)
            .context("couldn't clean out existing SDK from aws-sdk-rust")?;
        copy_sdk(&build_artifacts, aws_sdk)?;
        create_mirror_commit(&aws_sdk_repo, &commit)
            .context("couldn't commit SDK changes to aws-sdk-rust")?;
    }
    println!("Successfully synced {} commit(s)", commit_revs.len());

    // Get the last commit we synced so that we can set that for the next time this tool gets run
    let last_synced_commit = commit_revs
        .last()
        .expect("can't be empty because we'd have early returned");
    println!("Updating 'commit at last sync' to {}", last_synced_commit);

    // Update the file containing the commit hash
    set_last_synced_commit(&aws_sdk_repo, last_synced_commit).with_context(|| {
        format!(
            "couldn't write last synced commit hash ({}) to aws-sdk-rust/{}",
            last_synced_commit, COMMIT_HASH_FILENAME,
        )
    })?;
    // Commit the file containing the commit hash
    commit_last_synced_commit_file(&aws_sdk_repo)
        .context("couldn't commit the last synced commit hash file to aws-sdk-rust")?;

    println!(
        "Successfully synced all mirror commits to aws-sdk-rust/{}. Don't forget to push them",
        branch
    );

    Ok(())
}

/// Starting from a given commit, walk the tree to its `HEAD` in order to build a list of commits that we'll
/// need to sync. If you don't see the commits you're expecting, make sure the repo is up to date.
/// This function doesn't include the `since_commit` in the list since that commit was synced last time
/// this tool was run.
fn commits_to_be_applied(smithy_rs_repo: &Repository, since_commit: &Oid) -> Result<Vec<Oid>> {
    let rev_range = format!("{}..HEAD", since_commit);
    println!("Checking for smithy-rs commits in range {}", rev_range);

    let mut rev_walk = smithy_rs_repo.revwalk().context(here!())?;
    rev_walk.push_range(&rev_range).context(here!())?;

    let mut commit_revs = Vec::new();
    for rev in rev_walk {
        let rev = rev.context(here!())?;
        commit_revs.push(rev);
    }

    // Order the revs from earliest to latest
    commit_revs.reverse();

    Ok(commit_revs)
}

/// Read the file from aws-sdk-rust that tracks the last smithy-rs commit it was synced with.
/// Returns the hash of that commit.
fn get_last_synced_commit(repo_path: &Path) -> Result<Oid> {
    let path = repo_path.join(COMMIT_HASH_FILENAME);

    let mut file = OpenOptions::new()
        .read(true)
        .open(&path)
        .with_context(|| format!("couldn't open file at '{}'", path.to_string_lossy()))?;
    // Commit hashes are 40 chars long
    let mut commit_hash = String::with_capacity(40);
    file.read_to_string(&mut commit_hash)
        .with_context(|| format!("couldn't read file at '{}'", path.to_string_lossy()))?;

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
        .with_context(|| format!("Couldn't write commit hash to '{}'", path.to_string_lossy()))?;

    Ok(())
}

/// Run the necessary commands to build the SDK. On success, returns the path to the folder containing
/// the build artifacts.
fn build_sdk(smithy_rs_path: &Path) -> Result<PathBuf> {
    println!("\tbuilding the SDK...");
    let start = Instant::now();
    let gradlew = smithy_rs_path.join("gradlew");

    // The output of running this command isn't logged anywhere unless it fails
    let clean_command_output = Command::new(&gradlew)
        .arg("-Paws.fullsdk=true")
        .arg(":aws:sdk:clean")
        .current_dir(smithy_rs_path)
        .output()
        .with_context(|| {
            format!(
                "failed to execute '{} -Paws.fullsdk=true :aws:sdk:clean'",
                gradlew.to_string_lossy()
            )
        })?;

    if !clean_command_output.status.success() {
        let stderr = String::from_utf8_lossy(&clean_command_output.stderr);
        let stdout = String::from_utf8_lossy(&clean_command_output.stdout);

        println!("stdout:\n{}\n", stdout);
        println!("stderr:\n{}\n", stderr);

        bail!("command to clean previous build artifacts from smithy-rs before assembling the SDK returned an error")
    }

    let assemble_command_output = Command::new(&gradlew)
        .arg("-Paws.fullsdk=true")
        .arg(":aws:sdk:assemble")
        .current_dir(smithy_rs_path)
        .output()
        .with_context(|| {
            format!(
                "failed to execute '{} -Paws.fullsdk=true :aws:sdk:assemble'",
                gradlew.to_string_lossy()
            )
        })?;

    if !assemble_command_output.status.success() {
        let stderr = String::from_utf8_lossy(&clean_command_output.stderr);
        let stdout = String::from_utf8_lossy(&clean_command_output.stdout);

        println!("stdout:\n{}\n", stdout);
        println!("stderr:\n{}\n", stderr);

        bail!("command to assemble the SDK returned an error")
    }

    let build_artifact_path = smithy_rs_path.join("aws/sdk/build/aws-sdk");

    println!("\tsuccessfully built the SDK in {:?}", start.elapsed());
    Ok(build_artifact_path)
}

/// Delete any current SDK files in aws-sdk-rust. Run this before copying over new files.
fn clean_out_existing_sdk(aws_sdk_path: &Path) -> Result<()> {
    println!("\tcleaning out previously built SDK...");

    let sdk_path = format!("{}/sdk/*", aws_sdk_path.to_string_lossy());
    let remove_sdk_command_output = Command::new("rm")
        .arg("-rf")
        .arg(&sdk_path)
        .current_dir(aws_sdk_path)
        .output()?;
    if !remove_sdk_command_output.status.success() {
        bail!("failed to clean out the SDK folder at {}", sdk_path);
    }

    let examples_path = format!("{}/example/*", aws_sdk_path.to_string_lossy());
    let remove_examples_command_output = Command::new("rm")
        .arg("-rf")
        .arg(&examples_path)
        .current_dir(aws_sdk_path)
        .output()?;
    if !remove_examples_command_output.status.success() {
        bail!(
            "failed to clean out the examples folder at {}",
            examples_path
        );
    }

    println!("\tsuccessfully cleaned out previously built SDK");
    Ok(())
}

/// Use `cp -r` to recursively copy all files and folders from the smithy-rs build artifacts folder
/// to the aws-sdk-rust repo folder
fn copy_sdk(from_path: &Path, to_path: &Path) -> Result<()> {
    println!("\tcopying built SDK...");

    let copy_sdk_command_output = Command::new("cp")
        .arg("-r")
        .arg(&from_path)
        .arg(&to_path)
        .output()?;
    if !copy_sdk_command_output.status.success() {
        bail!(
            "failed to copy the built SDK from {} to {}",
            from_path.to_string_lossy(),
            to_path.to_string_lossy()
        );
    }

    println!("\tsuccessfully copied built SDK");
    Ok(())
}

/// Find the last commit made to a repo
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
/// attached to it to create a commit in aws-sdk-rust.
fn create_mirror_commit(aws_sdk_repo: &Repository, based_on_commit: &Commit) -> Result<()> {
    println!("\tcreating mirror commit...");

    let mut index = aws_sdk_repo.index().context(here!())?;
    // The equivalent of `git add .`
    index
        .add_all(["."].iter(), IndexAddOption::DEFAULT, None)
        .context(here!())?;
    let oid = index.write_tree().context(here!())?;
    let parent_commit = find_last_commit(&aws_sdk_repo).context(here!())?;
    let tree = aws_sdk_repo.find_tree(oid).context(here!())?;

    let _ = aws_sdk_repo
        .commit(
            Some("HEAD"),
            &based_on_commit.author(),
            &based_on_commit.committer(),
            based_on_commit.message().unwrap_or_default(),
            &tree,
            &[&parent_commit],
        )
        .context(here!())?;

    println!("\tsuccessfully created mirror commit");

    Ok(())
}

/// Commit the file in aws-sdk-rust that tracks what smithy-rs commit the SDK was last built from
fn commit_last_synced_commit_file(aws_sdk_repo: &Repository) -> Result<()> {
    let mut index = aws_sdk_repo.index().context(here!())?;
    index
        .add_path(Path::new(COMMIT_HASH_FILENAME))
        .context(here!())?;
    let signature = Signature::now(BOT_NAME, BOT_EMAIL)?;
    let oid = index.write_tree().context(here!())?;
    let parent_commit = find_last_commit(&aws_sdk_repo).context(here!())?;
    let tree = aws_sdk_repo.find_tree(oid).context(here!())?;

    let _ = aws_sdk_repo
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            &format!("update: {} with last synced commit", COMMIT_HASH_FILENAME),
            &tree,
            &[&parent_commit],
        )
        .context(here!())?;

    Ok(())
}

/// `git checkout` the branch of aws-sdk-rust that we want to mirror commits to (defaults to `next`)
fn checkout_branch_to_sync_to(aws_sdk_repo: &Repository, aws_sdk_branch: &str) -> Result<()> {
    aws_sdk_repo
        .find_remote("origin")?
        .fetch(&["main"], None, None)?;
    let (object, _) = aws_sdk_repo.revparse_ext(aws_sdk_branch).context(here!())?;

    aws_sdk_repo.checkout_tree(&object, None).context(here!())?;

    Ok(())
}

/// `git checkout` a commit from smithy-rs that we're going to mirror to aws-sdk-rust
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
