/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::{Context, Result};
use clap::Parser;
use sdk_sync::init_tracing;
use sdk_sync::sync::Sync;
use smithy_rs_tool_common::macros::here;
use std::path::PathBuf;

/// A CLI tool to replay commits from smithy-rs, generate code, and commit that code to aws-rust-sdk.
#[derive(Parser, Debug)]
#[clap(name = "sdk-sync")]
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
    init_tracing();
    let args = Args::parse();

    let sync = Sync::new(
        &args.aws_doc_sdk_examples.canonicalize().context(here!())?,
        &args.aws_sdk_rust.canonicalize().context(here!())?,
        &args.smithy_rs.canonicalize().context(here!())?,
    )?;

    sync.sync().map_err(|e| e.context("The sync failed"))
}
