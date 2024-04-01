/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::{
    repo::Repo,
    tag::{previous_release_tag, release_tags},
    PatchRuntime, PatchRuntimeWith,
};
use anyhow::{bail, Context, Result};
use camino::Utf8Path;
use cargo_toml::Manifest;
use indicatif::{ProgressBar, ProgressStyle};
use smithy_rs_tool_common::command::sync::CommandExt;
use std::{fs, time::Duration};
use toml_edit::DocumentMut;

pub fn patch(args: PatchRuntime) -> Result<()> {
    let smithy_rs = step("Resolving smithy-rs", || {
        Repo::new(args.smithy_rs_path.as_deref())
    })?;
    if is_dirty(&smithy_rs)? {
        bail!("smithy-rs has a dirty working tree. Aborting.");
    }

    let aws_sdk_rust = step("Resolving aws-sdk-rust", || Repo::new(Some(&args.sdk_path)))?;
    if is_dirty(&aws_sdk_rust)? {
        bail!("aws-sdk-rust has a dirty working tree. Aborting.");
    }

    // Use aws:sdk:assemble to generate both the smithy-rs runtime and AWS SDK
    // runtime crates with the correct version numbers.
    step("Generating an AWS SDK", || {
        smithy_rs
            .cmd(
                "./gradlew",
                // limit services down to minimum required to reduce generation time
                ["-Paws.services=+sts,+sso,+ssooidc", "aws:sdk:assemble"],
            )
            .expect_success_output("assemble SDK")
    })?;

    patch_with(PatchRuntimeWith {
        sdk_path: args.sdk_path,
        runtime_crate_path: smithy_rs.root.join("aws/sdk/build/aws-sdk/sdk"),
        previous_release_tag: args.previous_release_tag,
    })?;

    Ok(())
}

pub fn patch_with(args: PatchRuntimeWith) -> Result<()> {
    let aws_sdk_rust = step("Resolving aws-sdk-rust", || Repo::new(Some(&args.sdk_path)))?;
    if is_dirty(&aws_sdk_rust)? {
        bail!("aws-sdk-rust has a dirty working tree. Aborting.");
    }

    // Make sure the aws-sdk-rust repo is on the correct release tag
    let release_tags = step("Resolving aws-sdk-rust release tags", || {
        release_tags(&aws_sdk_rust)
    })?;
    let previous_release_tag = step("Resolving release tag", || {
        previous_release_tag(
            &aws_sdk_rust,
            &release_tags,
            args.previous_release_tag.as_deref(),
        )
    })?;
    step("Checking out release tag", || {
        aws_sdk_rust
            .git(["checkout", previous_release_tag.as_str()])
            .expect_success_output("check out release tag in aws-sdk-rust")
    })?;

    // Patch the new runtime crates into the old SDK
    step("Applying version-only dependencies", || {
        apply_version_only_dependencies(&aws_sdk_rust)
    })?;
    step("Patching aws-sdk-rust root Cargo.toml", || {
        let crates_to_patch =
            remove_unchanged_dependencies(&aws_sdk_rust, &args.runtime_crate_path)?;
        patch_workspace_cargo_toml(&aws_sdk_rust, &args.runtime_crate_path, crates_to_patch)
    })?;
    step("Running cargo update", || {
        aws_sdk_rust
            .cmd("cargo", ["update"])
            .expect_success_output("cargo update")
    })?;

    Ok(())
}

fn apply_version_only_dependencies(aws_sdk_rust: &Repo) -> Result<()> {
    aws_sdk_rust
        .cmd(
            "sdk-versioner",
            [
                "use-version-dependencies",
                "--versions-toml",
                "versions.toml",
                "sdk",
            ],
        )
        .expect_success_output("run sdk-versioner")?;
    Ok(())
}

/// Determine if a given crate has a new version vs. the release we're comparing
fn crate_version_has_changed(
    crate_name: &str,
    aws_sdk_rust: &Repo,
    runtime_crate_path: &Utf8Path,
) -> Result<bool> {
    let sdk_cargo_toml = aws_sdk_rust
        .root
        .join("sdk")
        .join(crate_name)
        .join("Cargo.toml");
    let to_patch_cargo_toml = runtime_crate_path.join(crate_name).join("Cargo.toml");
    if !sdk_cargo_toml.exists() {
        tracing::trace!("`{crate_name}` is a new crate, so there is nothing to patch.");
        // This is a new runtime crate, so there is nothing to patch.
        return Ok(false);
    }
    assert!(
        to_patch_cargo_toml.exists(),
        "{:?} did not exist!",
        to_patch_cargo_toml
    );
    let sdk_cargo_toml = Manifest::from_path(sdk_cargo_toml).context("could not parse")?;
    let to_patch_toml = Manifest::from_path(to_patch_cargo_toml).context("could not parse")?;
    Ok(sdk_cargo_toml.package().version() != to_patch_toml.package().version())
}
fn patch_workspace_cargo_toml(
    aws_sdk_rust: &Repo,
    runtime_crate_path: &Utf8Path,
    crates_to_patch: impl Iterator<Item = String>,
) -> Result<()> {
    let patch_sections = crates_to_patch
        .map(|crate_name| {
            let path = runtime_crate_path.join(&crate_name);
            assert!(
                path.exists(),
                "tried to reference a crate that did not exist!"
            );
            format!(
                "{crate_name} = {{ path = '{}' }}",
                path.canonicalize_utf8().unwrap()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let patch_section = format!("\n[patch.crates-io]\n{patch_sections}");

    let manifest_path = aws_sdk_rust.root.join("Cargo.toml");
    tracing::trace!("patching {manifest_path}");
    let mut manifest_content =
        fs::read_to_string(&manifest_path).context("failed to read aws-sdk-rust/Cargo.toml")?;
    manifest_content.push_str(&patch_section);
    fs::write(&manifest_path, &manifest_content)
        .context("failed to write aws-sdk-rust/Cargo.toml")?;
    Ok(())
}

/// Removes Path dependencies referring to unchanged crates & returns a list of crates to patch
fn remove_unchanged_dependencies(
    aws_sdk_rust: &Repo,
    runtime_crate_path: &Utf8Path,
) -> Result<impl Iterator<Item = String>> {
    let all_crates = fs::read_dir(runtime_crate_path)
        .context(format!(
            "could list crates in directory {:?}",
            runtime_crate_path
        ))?
        .map(|dir| dir.unwrap().file_name())
        .map(|osstr| osstr.into_string().expect("invalid utf-8 directory"))
        .collect::<Vec<_>>();

    let (crates_to_patch, unchanged_crates): (Vec<_>, Vec<_>) =
        all_crates.clone().into_iter().partition(|crate_dir| {
            crate_version_has_changed(crate_dir, aws_sdk_rust, runtime_crate_path)
                .expect("failed to determine change-status")
        });

    for patched_crate in &all_crates {
        tracing::trace!("removing unchanged path dependencies for {patched_crate}");
        remove_unchanged_path_dependencies(runtime_crate_path, &unchanged_crates, patched_crate)?;
    }
    Ok(crates_to_patch
        .into_iter()
        .filter(|crte| crte.starts_with("aws-")))
}

/// Remove `path = ...` from the dependency section for unchanged crates
///
/// If we leave these path dependencies in, we'll get an error when we try to patch because the
/// version numbers are the same.
fn remove_unchanged_path_dependencies(
    runtime_crate_path: &Utf8Path,
    unchanged_crates: &[String],
    patched_crate: &String,
) -> Result<()> {
    let path = runtime_crate_path.join(patched_crate).join("Cargo.toml");
    let manifest = Manifest::from_path(&path)?;
    let mut mutable_manifest = fs::read_to_string(&path)
        .context("failed to read file")
        .context(path.clone())?
        .parse::<DocumentMut>()
        .context("invalid toml in manifest!")?;
    let mut updates = false;
    let sections = [
        (manifest.dependencies, "dependencies"),
        (manifest.dev_dependencies, "dev-dependencies"),
    ];
    for (deps_set, key) in sections {
        for (dependency_name, dependency_metadata) in deps_set.iter() {
            if unchanged_crates.iter().any(|crate_name| {
                crate_name.as_str()
                    == dependency_metadata
                        .package()
                        .unwrap_or(dependency_name.as_str())
            }) {
                let it = &mut mutable_manifest[key][dependency_name];
                match it.as_table_like_mut() {
                    Some(table_like) => table_like.remove("path"),
                    None => {
                        panic!("crate `{patched_crate}` depends on crate `{dependency_name}` crate by version instead of by path. Please update it to use path dependencies for all runtime crates.");
                    }
                };
                updates = true
            }
        }
    }
    if updates {
        fs::write(&path, mutable_manifest.to_string()).context("failed to write back manifest")?
    }
    Ok(())
}

fn is_dirty(repo: &Repo) -> Result<bool> {
    let result = repo
        .git(["status", "--porcelain"])
        .expect_success_output("git status")?;
    Ok(!result.trim().is_empty())
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
