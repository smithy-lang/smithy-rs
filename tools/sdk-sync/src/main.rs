/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::{Context, Result};
use clap::Parser;
use sdk_sync::init_tracing;
use sdk_sync::sync::gen::CodeGenSettings;
use sdk_sync::sync::Sync;
use smithy_rs_tool_common::macros::here;
use std::path::PathBuf;
use systemstat::{Platform, System};
use tracing::info;

const CODEGEN_MIN_RAM_REQUIRED_GB: usize = 2;

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

    /// Number of threads that `sdk-sync` will use. Defaults to the physical number of CPUs,
    /// or the available RAM divided by the RAM required for codegen. Whichever is smaller.
    #[clap(long)]
    sync_threads: Option<usize>,
    /// The Java parallelism (corresponding to the `java.util.concurrent.ForkJoinPool.common.parallelism`
    /// system property) to use for Smithy codegen. Defaults to 1.
    #[clap(long)]
    smithy_parallelism: Option<usize>,
    /// The maximum Java heap space (in megabytes) that the Gradle daemon is allowed to use during code generation.
    #[clap(long)]
    max_gradle_heap_megabytes: Option<usize>,
    /// The maximum Java metaspace (in megabytes) that the Gradle daemon is allowed to use during code generation.
    #[clap(long)]
    max_gradle_metaspace_megabytes: Option<usize>,
}

impl Args {
    fn codegen_settings(&self) -> CodeGenSettings {
        let defaults = CodeGenSettings::default();
        CodeGenSettings {
            smithy_parallelism: self
                .smithy_parallelism
                .unwrap_or(defaults.smithy_parallelism),
            max_gradle_heap_megabytes: self
                .max_gradle_heap_megabytes
                .unwrap_or(defaults.max_gradle_heap_megabytes),
            max_gradle_metaspace_megabytes: self
                .max_gradle_metaspace_megabytes
                .unwrap_or(defaults.max_gradle_metaspace_megabytes),
            aws_models_path: Some(self.aws_sdk_rust.join("aws-models")),
        }
    }
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

    let available_ram_gb = available_ram_gb();
    let num_cpus = num_cpus::get_physical();
    info!("Available RAM (GB): {available_ram_gb}");
    info!("Num physical CPUs: {num_cpus}");

    let sync_threads = if let Some(sync_threads) = args.sync_threads {
        sync_threads
    } else {
        (available_ram_gb / CODEGEN_MIN_RAM_REQUIRED_GB)
            .max(1) // Must use at least 1 thread
            .min(num_cpus) // Don't exceed the number of physical CPUs
    };
    info!("Sync thread pool size: {sync_threads}");

    rayon::ThreadPoolBuilder::new()
        .num_threads(sync_threads)
        .build_global()
        .unwrap();

    let sync = Sync::new(
        &args.aws_doc_sdk_examples.canonicalize().context(here!())?,
        &args.aws_sdk_rust.canonicalize().context(here!())?,
        &args.smithy_rs.canonicalize().context(here!())?,
        args.codegen_settings(),
    )?;

    sync.sync().map_err(|e| e.context("The sync failed"))
}

fn available_ram_gb() -> usize {
    let sys = System::new();
    let memory = sys.memory().expect("determine free memory");
    (memory.free.as_u64() / 1024 / 1024 / 1024) as usize
}
