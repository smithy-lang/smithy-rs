/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::{
    repo::Repo,
    tag::{previous_release_tag, release_tags},
    PatchRuntime,
};
use anyhow::{bail, Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use smithy_rs_tool_common::command::sync::CommandExt;
use std::{fs, time::Duration};

pub fn patch(args: PatchRuntime) -> Result<()> {
    let smithy_rs = step("Resolving smithy-rs", || {
        Repo::new(args.smithy_rs_path.as_deref())
    })?;
    let aws_sdk_rust = step("Resolving aws-sdk-rust", || Repo::new(Some(&args.sdk_path)))?;
    if is_dirty(&smithy_rs)? {
        bail!("smithy-rs has a dirty working tree. Aborting.");
    }
    if is_dirty(&aws_sdk_rust)? {
        bail!("aws-sdk-rust has a dirty working tree. Aborting.");
    }

    step(
        "Patching smithy-rs/gradle.properties with given crate version numbers",
        || patch_gradle_properties(&smithy_rs, &args),
    )?;

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
        patch_workspace_cargo_toml(&aws_sdk_rust, &smithy_rs)
    })?;
    step("Running cargo update", || {
        aws_sdk_rust
            .cmd("cargo", ["update"])
            .expect_success_output("cargo update")
    })?;

    Ok(())
}

fn patch_gradle_properties(smithy_rs: &Repo, args: &PatchRuntime) -> Result<()> {
    let props_path = smithy_rs.root.join("gradle.properties");
    let props =
        fs::read_to_string(&props_path).context("failed to read smithy-rs/gradle.properties")?;
    let mut new_props = String::with_capacity(props.len());
    for line in props.lines() {
        if line.starts_with("smithy.rs.runtime.crate.stable.version=") {
            new_props.push_str(&format!(
                "smithy.rs.runtime.crate.stable.version={}",
                args.stable_crate_version
            ));
        } else if line.starts_with("smithy.rs.runtime.crate.unstable.version=") {
            new_props.push_str(&format!(
                "smithy.rs.runtime.crate.unstable.version={}",
                args.unstable_crate_version
            ));
        } else {
            new_props.push_str(line);
        }
        new_props.push('\n');
    }
    fs::write(&props_path, new_props).context("failed to write smithy-rs/gradle.properties")?;
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

fn patch_workspace_cargo_toml(aws_sdk_rust: &Repo, smithy_rs: &Repo) -> Result<()> {
    let path = smithy_rs.root.join("aws/sdk/build/aws-sdk/sdk");
    let crates_to_patch = fs::read_dir(&path)
        .context(format!("could list crates in directory {:?}", path))?
        .map(|dir| dir.unwrap().file_name())
        .map(|osstr| osstr.into_string().expect("invalid utf-8 directory"))
        .filter(|name| name.starts_with("aws-"))
        .collect::<Vec<_>>();

    let patch_sections = crates_to_patch
        .iter()
        .map(|crte| {
            let path = path.join(crte);
            assert!(
                path.exists(),
                "tried to reference a crate that did not exist!"
            );
            format!(
                "{crte} = {{ path = '{}' }}",
                path.canonicalize_utf8().unwrap()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let patch_section = format!("\n[patch.crates-io]\n{patch_sections}");

    let manifest_path = aws_sdk_rust.root.join("Cargo.toml");
    let mut manifest_content =
        fs::read_to_string(&manifest_path).context("failed to read aws-sdk-rust/Cargo.toml")?;
    manifest_content.push_str(&patch_section);
    fs::write(&manifest_path, &manifest_content)
        .context("failed to write aws-sdk-rust/Cargo.toml")?;
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
