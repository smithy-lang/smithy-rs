/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Result};
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
    name = "release-dryrun",
    about = "CLI tool to recursively update SDK/Smithy crate references in Cargo.toml files",
    version
)]
#[allow(clippy::enum_variant_names)] // Want the "use" prefix in the CLI subcommand names for clarity
enum Args {
    DryRunSdk(DryRunSdk),
}

fn main() -> Result<()> {
    let args = Args::parse();
    match args {
        Args::DryRunSdk(args) => dry_run_sdk(args)?,
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
    spinner.set_style(ProgressStyle::with_template(&format!("{{msg}} {{elapsed}}")).unwrap());
    spinner.finish_with_message(format!("{check} {message}"));
    result
}

fn dry_run_sdk(args: DryRunSdk) -> Result<()> {
    step("Checking out SDK tag", || {
        Command::new("git")
            .arg("checkout")
            .arg(&args.rust_sdk_tag)
            .current_dir(&args.sdk_path)
            .output()?;
        Ok(())
    })?;

    step("Applying version-only dependencies", || {
        Command::new("sdk-versioner")
            .args([
                "use-version-dependencies",
                "--versions-toml",
                "versions.toml",
                "sdk",
            ])
            .current_dir(&args.sdk_path)
            .output()?;

        Command::new("git")
            .args(["checkout", "-B", "smithy-release-dryrun"])
            .current_dir(&args.sdk_path)
            .output()?;
        Command::new("git")
            .args([
                "commit",
                "-am",
                "removing path dependencies to allow patching",
            ])
            .current_dir(&args.sdk_path)
            .output()?;
        Ok(())
    })?;

    let patches = step("computing patches", || {
        let crates_to_patch =
            std::fs::read_dir(Path::new(&args.smithy_rs_release).join("crates-to-publish"))?
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
    step("apply patches to workspace Cargo.toml", || {
        let workspace_cargo_toml = Path::new(&args.sdk_path).join("Cargo.toml");
        if !workspace_cargo_toml.exists() {
            bail!(
                "Could not find the workspace Cargo.toml to patch {:?}",
                workspace_cargo_toml
            );
        }
        let current_contents = std::fs::read_to_string(&workspace_cargo_toml)?;
        std::fs::write(
            workspace_cargo_toml,
            format!("{current_contents}\n{patches}"),
        )?;
        Command::new("git")
            .args(["commit", "-am", "patching workspace Cargo.toml"])
            .current_dir(&args.sdk_path)
            .output()?;
        Ok(())
    })?;
    println!("{:?} has been updated to build against patches. Use `cargo update` to recompute the dependencies.", &args.sdk_path);
    Ok(())
}
