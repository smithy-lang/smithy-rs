/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// this tool is simple and works at what it does
// potential improvements:
// - support the release of rust-runtime crates
// - support patching the users system-wide ~/.cargo/config.toml

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};

#[derive(Parser, Debug)]
struct DryRunSdk {
    /// Path to the aws-sdk-rust repo
    #[clap(long)]
    pub sdk_path: PathBuf,

    /// Tag of the aws-sdk-rust repo to dry-run against
    #[clap(long)]
    pub rust_sdk_tag: String,

    /// Path to the artifact produced by the release-dry run
    #[clap(long)]
    pub smithy_rs_release: PathBuf,
}

#[derive(Parser, Debug)]
#[clap(
    name = "runtime-release-dryrun",
    about = "CLI tool to prepare the aws-sdk-rust to test the result of releasing a new set of runtime crates.",
    version
)]
#[allow(clippy::enum_variant_names)] // Want the "use" prefix in the CLI subcommand names for clarity
enum Args {
    /// Dry run a smithy-rs release against a rust SDK release
    ///
    /// You will need:
    /// 1. An `aws-sdk-rust` repo with a clean working tree
    /// 2. A directory containing the artifacts from a smithy-rs release dry run. This is an artifact
    ///    named `artifacts-generate-smithy-rs-release` from the GH action (e.g. https://github.com/smithy-lang/smithy-rs/actions/runs/7200898068)
    /// 3. An `aws-sdk-rust` release tag you want to test against
    ///
    /// After running the tool, you might want to do something like `cargo test` in `s3`. Make sure
    /// to run `cargo update` to pull in the new dependencies. Use `cargo tree` to confirm you're
    /// actually consuming the new versions.
    #[clap(verbatim_doc_comment)]
    DryRunSdk(DryRunSdk),
}

fn main() -> Result<()> {
    let args = Args::parse();
    match args {
        Args::DryRunSdk(args) => dry_run_sdk(args).map_err(|err| {
            // workaround an indicatif (bug?) where one character is stripped from output on the error message
            eprintln!(" ");
            err
        })?,
    }
    Ok(())
}

fn step<T>(message: &'static str, step: impl FnOnce() -> Result<T>) -> Result<T> {
    let spinner = ProgressBar::new_spinner()
        .with_message(message)
        .with_style(ProgressStyle::with_template("{spinner} {msg} {elapsed}").unwrap());
    spinner.enable_steady_tick(Duration::from_millis(100));
    let result = step();
    let check = match &result {
        Ok(_) => "✅",
        Err(_) => "❌",
    };
    spinner.set_style(ProgressStyle::with_template("{msg} {elapsed}").unwrap());
    spinner.finish_with_message(format!("{check} {message}"));
    result
}

fn run(command: &mut Command) -> anyhow::Result<()> {
    let status = command.output()?;
    if !status.status.success() {
        bail!(
            "command `{:?}` failed:\n{}{}",
            command,
            String::from_utf8_lossy(&status.stdout),
            String::from_utf8_lossy(&status.stderr)
        );
    }
    Ok(())
}

fn dry_run_sdk(args: DryRunSdk) -> Result<()> {
    step("Checking out SDK tag", || {
        run(Command::new("git")
            .arg("checkout")
            .arg(&args.rust_sdk_tag)
            .current_dir(&args.sdk_path))
        .context("failed to checkout aws-sdk-rust revision")?;
        Ok(())
    })?;

    // By default the SDK dependencies also include a path component. This prevents
    // patching from working
    step("Applying version-only dependencies", || {
        run(Command::new("sdk-versioner")
            .args([
                "use-version-dependencies",
                "--versions-toml",
                "versions.toml",
                "sdk",
            ])
            .current_dir(&args.sdk_path))?;

        run(Command::new("git")
            .args(["checkout", "-B", "smithy-release-dryrun"])
            .current_dir(&args.sdk_path))?;
        run(Command::new("git")
            .args([
                "commit",
                "-am",
                "removing path dependencies to allow patching",
            ])
            .current_dir(&args.sdk_path))?;
        Ok(())
    })?;

    let patches = step("computing patches", || {
        let path = Path::new(&args.smithy_rs_release).join("crates-to-publish");
        let crates_to_patch = std::fs::read_dir(&path)
            .context(format!("could list crates in directory {:?}", path))?
            .map(|dir| dir.unwrap().file_name())
            .map(|osstr| osstr.into_string().expect("invalid utf-8 directory"))
            .collect::<Vec<_>>();

        let patch_sections = crates_to_patch
            .iter()
            .map(|crte| {
                let path = Path::new(&args.smithy_rs_release)
                    .join("crates-to-publish")
                    .join(crte);
                assert!(
                    path.exists(),
                    "tried to reference a crate that did not exist!"
                );
                format!(
                    "{crte} = {{ path = '{}' }}",
                    path.canonicalize().unwrap().to_str().unwrap()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        Ok(format!("[patch.crates-io]\n{patch_sections}"))
    })?;

    // Note: in the future we could also automatically apply this to the system wide ~/.cargo/config.toml
    step("apply patches to workspace Cargo.toml", || {
        let workspace_cargo_toml = Path::new(&args.sdk_path).join("Cargo.toml");
        if !workspace_cargo_toml.exists() {
            bail!(
                "Could not find the workspace Cargo.toml to patch {:?}",
                workspace_cargo_toml
            );
        }
        let current_contents = std::fs::read_to_string(&workspace_cargo_toml)
            .context("could not read workspace cargo.toml")?;
        std::fs::write(
            workspace_cargo_toml,
            format!("{current_contents}\n{patches}"),
        )?;
        run(Command::new("git")
            .args(["commit", "-am", "patching workspace Cargo.toml"])
            .current_dir(&args.sdk_path))?;
        Ok(())
    })?;
    println!("{:?} has been updated to build against patches. Use `cargo update` to recompute the dependencies.", &args.sdk_path);
    Ok(())
}
