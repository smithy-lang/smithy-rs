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
use camino::Utf8PathBuf;
use cargo_toml::Manifest;
use indicatif::{ProgressBar, ProgressStyle};
use smithy_rs_tool_common::{command::sync::CommandExt, package::Package};
use std::{fs, time::Duration};
use toml_edit::{DocumentMut, Item};

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

    patch_with(PatchRuntimeWith {
        sdk_path: args.sdk_path,
        runtime_crate_path: vec![
            smithy_rs.root.join("rust-runtime"),
            smithy_rs.root.join("aws/rust-runtime"),
        ],
        previous_release_tag: args.previous_release_tag,
        no_checkout_sdk_release: args.no_checkout_sdk_release,
    })?;

    Ok(())
}

pub fn patch_with(args: PatchRuntimeWith) -> Result<()> {
    let aws_sdk_rust = step("Resolving aws-sdk-rust", || Repo::new(Some(&args.sdk_path)))?;
    if is_dirty(&aws_sdk_rust)? {
        bail!("aws-sdk-rust has a dirty working tree. Aborting.");
    }

    if !args.no_checkout_sdk_release {
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
    }

    // Patch the new runtime crates into the old SDK
    step("Applying version-only dependencies", || {
        apply_version_only_dependencies(&aws_sdk_rust)
    })?;
    step("Patching aws-sdk-rust root Cargo.toml", || {
        let crates_to_patch =
            remove_unchanged_dependencies(&aws_sdk_rust, &args.runtime_crate_path)?;
        patch_workspace_cargo_toml(&aws_sdk_rust, crates_to_patch)
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
fn crate_version_has_changed(runtime_crate: &Package, aws_sdk_rust: &Repo) -> Result<bool> {
    let sdk_cargo_toml = aws_sdk_rust
        .root
        .join("sdk")
        .join(&runtime_crate.handle.name)
        .join("Cargo.toml");
    let to_patch_cargo_toml = &runtime_crate.manifest_path;
    if !sdk_cargo_toml.exists() {
        tracing::trace!(
            "`{}` is a new crate, so there is nothing to patch.",
            runtime_crate.handle
        );
        // This is a new runtime crate, so there is nothing to patch.
        return Ok(false);
    }
    assert!(
        to_patch_cargo_toml.exists(),
        "{to_patch_cargo_toml:?} did not exist!"
    );
    let sdk_cargo_toml = Manifest::from_path(&sdk_cargo_toml)
        .context("could not parse SDK Cargo.toml")
        .context(sdk_cargo_toml)?;
    let to_patch_toml = Manifest::from_path(to_patch_cargo_toml)
        .context("could not parse Cargo.toml to patch")
        .with_context(|| to_patch_cargo_toml.display().to_string())?;
    Ok(sdk_cargo_toml.package().version() != to_patch_toml.package().version())
}

fn patch_workspace_cargo_toml(
    aws_sdk_rust: &Repo,
    crates_to_patch: impl Iterator<Item = Package>,
) -> Result<()> {
    let patch_sections = crates_to_patch
        .map(|runtime_crate| {
            format!(
                "{} = {{ path = '{}' }}",
                runtime_crate.handle.name,
                runtime_crate.crate_path.canonicalize().unwrap().display()
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
    runtime_crate_paths: &[Utf8PathBuf],
) -> Result<impl Iterator<Item = Package>> {
    let mut all_crates = Vec::new();
    for runtime_crate_path in runtime_crate_paths {
        let read_dir = fs::read_dir(runtime_crate_path).context(format!(
            "could list crates in directory {runtime_crate_path:?}"
        ))?;
        for directory in read_dir {
            let path = directory?.path();
            if let Some(runtime_crate) = Package::try_load_path(path)? {
                let name = &runtime_crate.handle.name;
                if name.starts_with("aws-") && name != "aws-config" {
                    all_crates.push(runtime_crate);
                }
            }
        }
    }

    let (crates_to_patch, unchanged_crates): (Vec<_>, Vec<_>) =
        all_crates.clone().into_iter().partition(|runtime_crate| {
            crate_version_has_changed(runtime_crate, aws_sdk_rust)
                .expect("failed to determine change-status")
        });

    let mut crates_to_patch = crates_to_patch;
    for pkg in &all_crates {
        if crate_is_new_and_used_by_existing_runtime(&crates_to_patch, pkg, aws_sdk_rust)
            .expect("failed to determine crate status")
        {
            tracing::trace!(
                "adding new crate `{}` to set of crates to be patched",
                pkg.handle
            );
            crates_to_patch.push(pkg.clone());
        }
    }

    for patched_crate in &all_crates {
        tracing::trace!(
            "removing unchanged path dependencies for {}",
            patched_crate.handle
        );
        remove_unchanged_path_dependencies(&unchanged_crates, patched_crate)?;
    }
    Ok(crates_to_patch.into_iter())
}

/// Check if a runtime crate is new and used by the new runtime.
///
/// This is an edge case where there is a new crate used by an existing runtime crate
/// such that failure to patch in the new crate we'll get an error because the new
/// crate won't be found. For these we need to add them to the list of crates to patch
/// in the root SDK Cargo.toml.
fn crate_is_new_and_used_by_existing_runtime(
    crates_to_patch: &Vec<Package>,
    runtime_crate: &Package,
    aws_sdk_rust: &Repo,
) -> Result<bool> {
    let sdk_cargo_toml = aws_sdk_rust
        .root
        .join("sdk")
        .join(&runtime_crate.handle.name)
        .join("Cargo.toml");

    if sdk_cargo_toml.exists() {
        // existing runtime crate
        return Ok(false);
    }

    // check if the new runtime crate is used by an existing crate that changed (i.e. is set to be patched)
    for pkg in crates_to_patch {
        let manifest = Manifest::from_path(pkg.manifest_path.clone())?;
        let used = manifest
            .dependencies
            .iter()
            .any(|(dep_name, dep_metadata)| {
                runtime_crate.handle.name.as_str()
                    == dep_metadata.package().unwrap_or(dep_name.as_str())
            });
        if used {
            tracing::trace!(
                "`{}` is a new crate and used by crate set to be patched: `{}`.",
                runtime_crate.handle,
                pkg.handle
            );
            return Ok(true);
        }
    }
    Ok(false)
}

/// Remove `path = ...` from the dependency section for unchanged crates,
/// and add version numbers for those where necessary.
///
/// If we leave these path dependencies in, we'll get an error when we try to patch because the
/// version numbers are the same.
fn remove_unchanged_path_dependencies(
    unchanged_crates: &[Package],
    patched_crate: &Package,
) -> Result<()> {
    let manifest_path = &patched_crate.manifest_path;
    let manifest = Manifest::from_path(manifest_path)?;
    let mut mutable_manifest = fs::read_to_string(manifest_path)
        .context("failed to read file")
        .with_context(|| manifest_path.display().to_string())?
        .parse::<DocumentMut>()
        .context("invalid toml in manifest!")?;
    let mut updates = false;
    let sections = [
        (manifest.dependencies, "dependencies"),
        (manifest.dev_dependencies, "dev-dependencies"),
    ];
    for (deps_set, key) in sections {
        for (dependency_name, dependency_metadata) in deps_set.iter() {
            let runtime_crate = unchanged_crates.iter().find(|rt_crate| {
                rt_crate.handle.name.as_str()
                    == dependency_metadata
                        .package()
                        .unwrap_or(dependency_name.as_str())
            });
            if let Some(runtime_crate) = runtime_crate {
                let it = &mut mutable_manifest[key][dependency_name];
                match it.as_table_like_mut() {
                    Some(table_like) => {
                        table_like.remove("path");
                        if !table_like.contains_key("version") {
                            table_like.insert(
                                "version",
                                Item::Value(runtime_crate.handle.expect_version().to_string().into()),
                            );
                        }
                    }
                    None => panic!(
                        "crate `{}` depends on crate `{dependency_name}` crate by version instead \
                        of by path. Please update it to use path dependencies for all runtime crates.",
                        patched_crate.handle
                    )
                };
                updates = true
            }
        }
    }
    if updates {
        fs::write(manifest_path, mutable_manifest.to_string())
            .context("failed to write back manifest")?
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
