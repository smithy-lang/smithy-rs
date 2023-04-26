/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::Result;
use flate2::read::GzDecoder;
use std::fs::File;
use std::process::Command;
use tar::Archive;
use tempfile::TempDir;

use crate_hasher::file_list::FileList;

fn assert_correct_aws_smithy_async_hash(file_list: &FileList) {
    pretty_assertions::assert_eq!(
        include_str!("./aws-smithy-async-2022-04-08-entries.txt"),
        file_list.entries()
    );
    assert_eq!(
        "5f54783ff6ca7a899476ab5fa5388d82847f5052d96d3cbb64eb6830e22b0762",
        file_list.sha256()
    );
}

#[test]
fn test_against_aws_smithy_async() -> Result<()> {
    let dir = TempDir::new()?.path().join("test_against_aws_smithy_async");

    let tar = GzDecoder::new(File::open("tests/aws-smithy-async-2022-04-08.tar.gz")?);
    let mut archive = Archive::new(tar);
    archive.unpack(&dir)?;

    let file_list = FileList::discover(&dir.as_path().join("aws-smithy-async"))?;
    assert_correct_aws_smithy_async_hash(&file_list);
    Ok(())
}

#[test]
fn test_against_aws_smithy_async_with_ignored_files() -> Result<()> {
    let dir = TempDir::new()?.path().join("test_against_aws_smithy_async");

    let tar = GzDecoder::new(File::open("tests/aws-smithy-async-2022-04-08.tar.gz")?);
    let mut archive = Archive::new(tar);
    archive.unpack(&dir)?;

    std::fs::create_dir(&dir.as_path().join("target"))?;
    std::fs::write(
        &dir.as_path().join("target/something"),
        b"some data that should be excluded",
    )?;

    let file_list = FileList::discover(&dir.as_path().join("aws-smithy-async"))?;
    assert_correct_aws_smithy_async_hash(&file_list);

    Ok(())
}

#[test]
fn test_against_aws_smithy_async_with_git_repo() -> Result<()> {
    let dir = TempDir::new()?.path().join("test_against_aws_smithy_async");

    let tar = GzDecoder::new(File::open("tests/aws-smithy-async-2022-04-08.tar.gz")?);
    let mut archive = Archive::new(tar);
    archive.unpack(&dir)?;

    // Create a git repository that should be excluded from the hash
    Command::new("git")
        .arg("init")
        .arg(".")
        .current_dir(&dir.as_path().join("aws-smithy-async"))
        .output()?;

    let file_list = FileList::discover(&dir.as_path().join("aws-smithy-async"))?;
    assert_correct_aws_smithy_async_hash(&file_list);

    Ok(())
}
