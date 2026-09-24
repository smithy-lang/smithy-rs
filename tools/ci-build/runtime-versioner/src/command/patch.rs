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
use semver::{Version, VersionReq};
use smithy_rs_tool_common::{
    command::sync::CommandExt,
    package::{Package, PackageHandle},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use toml_edit::{DocumentMut, InlineTable, Item, Table, TableLike, Value};

/// The dependency table names that can appear both at the root of a manifest and
/// underneath a `[target.<cfg>]` table.
const DEPENDENCY_TABLES: &[&str] = &["dependencies", "dev-dependencies", "build-dependencies"];

/// The crate that is routed through the (rewritten) old SDK release rather than smithy-rs.
const AWS_CONFIG: &str = "aws-config";

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
        allow_compatibility_transition: args.allow_compatibility_transition,
    })?;

    Ok(())
}

pub fn patch_with(args: PatchRuntimeWith) -> Result<()> {
    let transition = args.allow_compatibility_transition;
    if transition {
        print_compatibility_transition_waiver();
    }

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
    let expected_patches = step("Patching aws-sdk-rust root Cargo.toml", || {
        let crates_to_patch =
            remove_unchanged_dependencies(&aws_sdk_rust, &args.runtime_crate_path)?;
        if !transition {
            patch_workspace_cargo_toml(&aws_sdk_rust, &crates_to_patch)?;
            return Ok(Vec::new());
        }

        // Transition mode: the old SDK release pins the _previous_ compatibility
        // line of some of the crates being patched in, so those requirements have
        // to be rewritten before Cargo will use the patch at all.
        let scan = rewrite_old_sdk_requirements(aws_sdk_rust.root.as_std_path(), &crates_to_patch)?;
        report_rewrites(&scan);
        let old_aws_config = old_sdk_aws_config(&aws_sdk_rust)?;
        let expected_patches =
            select_expected_patches(&crates_to_patch, &scan, &old_aws_config.handle);
        patch_workspace_cargo_toml_transition(&aws_sdk_rust, &crates_to_patch, &old_aws_config)?;
        Ok(expected_patches)
    })?;
    step("Running cargo update", || {
        aws_sdk_rust
            .cmd("cargo", ["update"])
            .expect_success_output("cargo update")
    })?;
    if transition {
        step("Verifying the patch set was used", || {
            verify_expected_patches(&aws_sdk_rust, &expected_patches)
        })?;
        // Repeat the waiver at the end so that it is the last thing a human sees.
        print_compatibility_transition_waiver();
    }

    Ok(())
}

const WAIVER_RULE: &str =
    "================================================================================";

/// Prints the compatibility transition waiver.
///
/// This must be impossible to miss: a passing run in this mode does *not* mean
/// that compatibility with already-published consumers has been restored.
fn print_compatibility_transition_waiver() {
    eprintln!("{WAIVER_RULE}");
    eprintln!("!! COMPATIBILITY TRANSITION MODE -- --allow-compatibility-transition !!");
    eprintln!();
    eprintln!("This run rewrites version requirements in the checked-out old SDK release so");
    eprintln!("that they accept the crate versions being patched in, and routes `aws-config`");
    eprintln!("through that rewritten old SDK copy.");
    eprintln!();
    eprintln!("It therefore exercises the COORDINATED POST-RELEASE dependency graph: the");
    eprintln!("graph that exists only once every affected crate in this release has been");
    eprintln!("published together.");
    eprintln!();
    eprintln!("It explicitly WAIVES compatibility with:");
    eprintln!("  * the already-published old `aws-config`, which pins the previous");
    eprintln!("    compatibility line of the patched runtime crates, and");
    eprintln!("  * any partial update, where a consumer upgrades only some of these crates.");
    eprintln!();
    eprintln!("A passing run does NOT claim that compatibility with those consumers has been");
    eprintln!("restored. It only claims that the coordinated graph resolves, builds, and");
    eprintln!("passes the old SDK's tests.");
    eprintln!("{WAIVER_RULE}");
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

fn patch_workspace_cargo_toml(aws_sdk_rust: &Repo, crates_to_patch: &[Package]) -> Result<()> {
    let patch_sections = crates_to_patch
        .iter()
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

/// TOML-aware equivalent of [`patch_workspace_cargo_toml`] used only in transition mode.
///
/// The default code path appends raw text so that its output stays byte-for-byte
/// identical to previous releases of this tool. Transition mode has to merge an
/// additional entry (`aws-config`) and cope with an existing `[patch.crates-io]`
/// table, so it edits the document instead.
fn patch_workspace_cargo_toml_transition(
    aws_sdk_rust: &Repo,
    crates_to_patch: &[Package],
    old_aws_config: &Package,
) -> Result<()> {
    let mut entries = Vec::new();
    for runtime_crate in crates_to_patch {
        entries.push((
            runtime_crate.handle.name.clone(),
            canonical_path_string(&runtime_crate.crate_path)?,
        ));
    }
    // Route `aws-config` through the rewritten old SDK crate. The smithy-rs copy of
    // `aws-config` is never a patch source: it depends on generated SDK crates that
    // don't exist in this workspace.
    entries.push((
        AWS_CONFIG.to_string(),
        canonical_path_string(&old_aws_config.crate_path)?,
    ));

    let manifest_path = aws_sdk_rust.root.join("Cargo.toml");
    tracing::trace!("patching {manifest_path}");
    let manifest_content =
        fs::read_to_string(&manifest_path).context("failed to read aws-sdk-rust/Cargo.toml")?;
    let mut manifest = manifest_content
        .parse::<DocumentMut>()
        .context("invalid toml in aws-sdk-rust/Cargo.toml")?;
    upsert_patch_entries(&mut manifest, &entries)?;
    fs::write(&manifest_path, manifest.to_string())
        .context("failed to write aws-sdk-rust/Cargo.toml")?;
    Ok(())
}

fn canonical_path_string(path: &Path) -> Result<String> {
    let canonical = path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {path:?}"))?;
    Ok(canonical.display().to_string())
}

/// Inserts (or replaces) `[patch.crates-io]` entries in the given manifest.
fn upsert_patch_entries(manifest: &mut DocumentMut, entries: &[(String, String)]) -> Result<()> {
    let patch = manifest.entry("patch").or_insert({
        let mut table = Table::new();
        // Render as `[patch.crates-io]` rather than an empty `[patch]` header.
        table.set_implicit(true);
        Item::Table(table)
    });
    let patch = patch
        .as_table_like_mut()
        .context("`patch` in aws-sdk-rust/Cargo.toml is not a table")?;
    let crates_io = match patch.get_mut("crates-io") {
        Some(existing) => existing,
        None => {
            patch.insert("crates-io", Item::Table(Table::new()));
            patch
                .get_mut("crates-io")
                .expect("just inserted `crates-io`")
        }
    };
    let crates_io = crates_io
        .as_table_like_mut()
        .context("`patch.crates-io` in aws-sdk-rust/Cargo.toml is not a table")?;
    for (name, path) in entries {
        let mut source = InlineTable::new();
        source.insert("path", path.clone().into());
        if crates_io
            .insert(name, Item::Value(Value::InlineTable(source)))
            .is_some()
        {
            tracing::warn!("replaced the pre-existing `[patch.crates-io]` entry for `{name}`");
        }
    }
    Ok(())
}

/// Removes Path dependencies referring to unchanged crates & returns a list of crates to patch
fn remove_unchanged_dependencies(
    aws_sdk_rust: &Repo,
    runtime_crate_paths: &[Utf8PathBuf],
) -> Result<Vec<Package>> {
    let mut all_crates = Vec::new();
    for runtime_crate_path in runtime_crate_paths {
        let read_dir = fs::read_dir(runtime_crate_path).context(format!(
            "could list crates in directory {runtime_crate_path:?}"
        ))?;
        for directory in read_dir {
            let path = directory?.path();
            if let Some(runtime_crate) = Package::try_load_path(path)? {
                let name = &runtime_crate.handle.name;
                if name.starts_with("aws-") && name != AWS_CONFIG {
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
    Ok(crates_to_patch)
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

/// A single version requirement that transition mode rewrote.
#[derive(Clone, Debug, Eq, PartialEq)]
struct RequirementRewrite {
    manifest: String,
    table: String,
    dependency: String,
    package: String,
    from: String,
    to: String,
}

impl RequirementRewrite {
    fn describe(&self) -> String {
        let alias = if self.dependency == self.package {
            String::new()
        } else {
            format!(" (package = \"{}\")", self.package)
        };
        format!(
            "{}: [{}] {}{} `{}` -> `{}`",
            self.manifest, self.table, self.dependency, alias, self.from, self.to
        )
    }
}

/// The result of scanning (and rewriting) the old SDK's manifests.
#[derive(Debug, Default, Eq, PartialEq)]
struct OldSdkScan {
    /// Every requirement that was rewritten, in manifest order.
    rewrites: Vec<RequirementRewrite>,
    /// Patched crates that the old SDK depends on unconditionally.
    required: BTreeSet<String>,
    /// Patched crates that the old SDK only depends on via `optional = true` entries.
    optional_only: BTreeSet<String>,
}

impl OldSdkScan {
    /// Crates referenced only by optional dependency entries, which therefore may
    /// legitimately not appear in the resolved graph.
    fn optional_only_names(&self) -> BTreeSet<&str> {
        self.optional_only
            .iter()
            .filter(|name| !self.required.contains(*name))
            .map(String::as_str)
            .collect()
    }
}

/// Rewrites version requirements in the old SDK that would prevent the patch from being used.
///
/// `sdk-versioner` has already converted every SDK/Smithy dependency in the old
/// SDK to a version-only dependency, so at this point each of those entries carries
/// the version that was published with the old release. Cargo ignores a
/// `[patch.crates-io]` entry whose version doesn't satisfy the requirement it is
/// meant to replace, so any requirement that rejects the version being patched in
/// is rewritten to that version. Requirements that already accept it are untouched.
fn rewrite_old_sdk_requirements(
    aws_sdk_rust_root: &Path,
    crates_to_patch: &[Package],
) -> Result<OldSdkScan> {
    let patched_versions: BTreeMap<String, Version> = crates_to_patch
        .iter()
        .map(|c| (c.handle.name.clone(), c.handle.expect_version().clone()))
        .collect();

    let mut manifest_paths = Vec::new();
    discover_manifests(&mut manifest_paths, &aws_sdk_rust_root.join("sdk"))?;
    manifest_paths.sort();

    let mut scan = OldSdkScan::default();
    for manifest_path in manifest_paths {
        let label = manifest_path
            .strip_prefix(aws_sdk_rust_root)
            .unwrap_or(&manifest_path)
            .display()
            .to_string();
        let contents = fs::read_to_string(&manifest_path)
            .with_context(|| format!("failed to read {manifest_path:?}"))?;
        let mut manifest = contents
            .parse::<DocumentMut>()
            .with_context(|| format!("failed to parse {manifest_path:?}"))?;
        if rewrite_manifest_requirements(&label, &mut manifest, &patched_versions, &mut scan)? {
            fs::write(&manifest_path, manifest.to_string())
                .with_context(|| format!("failed to write {manifest_path:?}"))?;
        }
    }
    Ok(scan)
}

/// Recursively discovers `Cargo.toml` files under `dir` that belong to the old
/// SDK workspace.
///
/// A nested manifest with its own `[workspace]` table is a separate workspace
/// root. Neither it nor anything below it consumes the old SDK root's
/// `[patch.crates-io]`, so rewriting it would create requirements that the
/// transition patches cannot satisfy.
fn discover_manifests(manifests: &mut Vec<PathBuf>, dir: &Path) -> Result<()> {
    let manifest_path = dir.join("Cargo.toml");
    if manifest_path.is_file() {
        let contents = fs::read_to_string(&manifest_path)
            .with_context(|| format!("failed to read {manifest_path:?}"))?;
        let manifest = contents
            .parse::<DocumentMut>()
            .with_context(|| format!("failed to parse {manifest_path:?}"))?;
        if manifest.get("workspace").is_some() {
            tracing::debug!(
                "skipping nested workspace rooted at {}",
                manifest_path.display()
            );
            return Ok(());
        }
        manifests.push(manifest_path);
    }

    for entry in fs::read_dir(dir).with_context(|| format!("failed to list {dir:?}"))? {
        let path = entry
            .with_context(|| format!("failed to read a directory entry in {dir:?}"))?
            .path();
        if !path.is_dir() || path.file_name() == Some(OsStr::new("target")) {
            continue;
        }
        discover_manifests(manifests, &path)?;
    }
    Ok(())
}

/// Rewrites incompatible requirements in one manifest. Returns true if it changed.
fn rewrite_manifest_requirements(
    manifest_label: &str,
    manifest: &mut DocumentMut,
    patched_versions: &BTreeMap<String, Version>,
    scan: &mut OldSdkScan,
) -> Result<bool> {
    let mut changed = false;
    for table_name in DEPENDENCY_TABLES {
        if let Some(item) = manifest.get_mut(table_name) {
            let table = item
                .as_table_like_mut()
                .with_context(|| format!("`{table_name}` in {manifest_label} is not a table"))?;
            changed |= rewrite_dependency_table(
                manifest_label,
                table_name,
                table,
                patched_versions,
                scan,
            )?;
        }
    }
    // `[target.<cfg>.dependencies]` and friends.
    if let Some(target_item) = manifest.get_mut("target") {
        let targets = target_item
            .as_table_like_mut()
            .with_context(|| format!("`target` in {manifest_label} is not a table"))?;
        for (target_key, target_value) in targets.iter_mut() {
            let target_label = target_key.get().to_string();
            let target_table = target_value.as_table_like_mut().with_context(|| {
                format!("`target.{target_label}` in {manifest_label} is not a table")
            })?;
            for table_name in DEPENDENCY_TABLES {
                if let Some(item) = target_table.get_mut(table_name) {
                    let label = format!("target.{target_label}.{table_name}");
                    let table = item
                        .as_table_like_mut()
                        .with_context(|| format!("`{label}` in {manifest_label} is not a table"))?;
                    changed |= rewrite_dependency_table(
                        manifest_label,
                        &label,
                        table,
                        patched_versions,
                        scan,
                    )?;
                }
            }
        }
    }
    Ok(changed)
}

fn rewrite_dependency_table(
    manifest_label: &str,
    table_label: &str,
    dependencies: &mut dyn TableLike,
    patched_versions: &BTreeMap<String, Version>,
    scan: &mut OldSdkScan,
) -> Result<bool> {
    let mut changed = false;
    for (key, value) in dependencies.iter_mut() {
        let dependency = key.get().to_string();
        let package = real_package_name(&dependency, value);
        let intended = match patched_versions.get(&package) {
            Some(version) => version,
            // Not a crate we're patching in.
            None => continue,
        };
        if is_optional(value) {
            scan.optional_only.insert(package.clone());
        } else {
            scan.required.insert(package.clone());
        }

        let location = format!("`{table_label}.{dependency}` in {manifest_label}");
        let slot = requirement_slot(value, &location)?.with_context(|| {
            format!(
                "{location} targets patched crate `{package}` but has no version requirement after sdk-versioner converted the old SDK to version dependencies"
            )
        })?;
        let current = slot
            .as_str()
            .with_context(|| format!("{location} has a non-string `version`"))?
            .to_string();
        if requirement_accepts(&current, intended, &location)? {
            continue;
        }
        let replacement = intended.to_string();
        // Only the version requirement is replaced. `features`, `default-features`,
        // `optional`, and `package` (and the surrounding formatting/comments, as far
        // as toml_edit tracks them) are left exactly as they were.
        set_string_preserving_decor(slot, &replacement);
        scan.rewrites.push(RequirementRewrite {
            manifest: manifest_label.to_string(),
            table: table_label.to_string(),
            dependency,
            package,
            from: current,
            to: replacement,
        });
        changed = true;
    }
    Ok(changed)
}

/// Resolves the real crate name of a dependency entry, honoring `package = "..."` aliases.
fn real_package_name(dependency: &str, value: &Item) -> String {
    value
        .as_table_like()
        .and_then(|table| table.get("package"))
        .and_then(|package| package.as_str())
        .unwrap_or(dependency)
        .to_string()
}

fn is_optional(value: &Item) -> bool {
    value
        .as_table_like()
        .and_then(|table| table.get("optional"))
        .and_then(|optional| optional.as_bool())
        .unwrap_or(false)
}

/// Returns the value holding a dependency's version requirement, if it has one.
///
/// Handles all three forms a dependency can take: `dep = "1"`,
/// `dep = { version = "1" }`, and `[dependencies.dep]` with `version = "1"`.
fn requirement_slot<'a>(value: &'a mut Item, location: &str) -> Result<Option<&'a mut Value>> {
    if value.is_str() {
        return Ok(Some(
            value.as_value_mut().expect("`is_str` implies a value"),
        ));
    }
    if value.as_table_like().is_none() {
        bail!("{location} is neither a string nor a table, which Cargo does not allow");
    }
    let table = value
        .as_table_like_mut()
        .expect("checked that this is table-like");
    match table.get_mut("version") {
        None => Ok(None),
        Some(version) => {
            Ok(Some(version.as_value_mut().with_context(|| {
                format!("{location} has a non-value `version`")
            })?))
        }
    }
}

fn requirement_accepts(requirement: &str, version: &Version, location: &str) -> Result<bool> {
    let parsed = VersionReq::parse(requirement).with_context(|| {
        format!("failed to parse the version requirement `{requirement}` at {location}")
    })?;
    Ok(parsed.matches(version))
}

fn set_string_preserving_decor(slot: &mut Value, new_value: &str) {
    let decor = slot.decor().clone();
    let mut replacement = Value::from(new_value);
    *replacement.decor_mut() = decor;
    *slot = replacement;
}

fn report_rewrites(scan: &OldSdkScan) {
    if scan.rewrites.is_empty() {
        tracing::info!(
            "no old SDK version requirements needed rewriting for the compatibility transition"
        );
        return;
    }
    tracing::warn!(
        "rewrote {} old SDK version requirement(s) for the compatibility transition:",
        scan.rewrites.len()
    );
    for rewrite in &scan.rewrites {
        tracing::warn!("  {}", rewrite.describe());
    }
}

/// The old SDK's `aws-config`, which is the only `aws-config` transition mode will use.
fn old_sdk_aws_config(aws_sdk_rust: &Repo) -> Result<Package> {
    let path = aws_sdk_rust.root.join("sdk").join(AWS_CONFIG);
    Package::try_load_path(path.as_std_path())
        .with_context(|| format!("failed to load {path}"))?
        .with_context(|| format!("expected an `aws-config` crate at {path}"))
}

/// A patch entry that must actually be used by the resolved dependency graph.
#[derive(Clone, Debug, Eq, PartialEq)]
struct ExpectedPatch {
    name: String,
    version: String,
    /// When true, any registry-resolved copy of this package is also a failure,
    /// regardless of version.
    reject_any_registry_copy: bool,
}

/// Selects the patch entries that must be used by the resolved graph.
///
/// A patched crate is only expected when the old SDK actually depends on it. Crates
/// with no old SDK dependency edge (for example the optional observability and DNS
/// integrations, which nothing in the SDK depends on) are legitimately unused, and
/// Cargo will report them as unused patches.
fn select_expected_patches(
    crates_to_patch: &[Package],
    scan: &OldSdkScan,
    old_aws_config: &PackageHandle,
) -> Vec<ExpectedPatch> {
    let mut expected = Vec::new();
    let mut unreferenced = Vec::new();
    for runtime_crate in crates_to_patch {
        let name = &runtime_crate.handle.name;
        if scan.required.contains(name) {
            expected.push(ExpectedPatch {
                name: name.clone(),
                version: runtime_crate.handle.expect_version().to_string(),
                reject_any_registry_copy: false,
            });
        } else {
            unreferenced.push(name.as_str());
        }
    }
    if !unreferenced.is_empty() {
        let optional_only = scan.optional_only_names();
        let (optional, unused): (Vec<_>, Vec<_>) = unreferenced
            .into_iter()
            .partition(|name| optional_only.contains(name));
        if !unused.is_empty() {
            tracing::info!(
                "these patched crates have no old SDK dependency edge and may go unused: {}",
                unused.join(", ")
            );
        }
        if !optional.is_empty() {
            tracing::info!(
                "these patched crates are only referenced by optional dependencies and may go unused: {}",
                optional.join(", ")
            );
        }
    }
    // `aws-config` is routed through the rewritten old SDK copy, so the published
    // `aws-config` from any registry version must not survive in the graph.
    expected.push(ExpectedPatch {
        name: old_aws_config.name.clone(),
        version: old_aws_config.expect_version().to_string(),
        reject_any_registry_copy: true,
    });
    expected
}

/// A `[[package]]` entry from a `Cargo.lock`.
#[derive(Clone, Debug, Eq, PartialEq)]
struct LockPackage {
    name: String,
    version: String,
    /// `None` for path (patched or workspace) packages.
    source: Option<String>,
}

fn verify_expected_patches(aws_sdk_rust: &Repo, expected: &[ExpectedPatch]) -> Result<()> {
    let lock_path = aws_sdk_rust.root.join("Cargo.lock");
    let contents =
        fs::read_to_string(&lock_path).with_context(|| format!("failed to read {lock_path}"))?;
    let packages = parse_lock_packages(&contents)?;
    validate_expected_patches(&packages, expected)
}

/// Parses the `[[package]]` entries out of a `Cargo.lock`.
///
/// This reads the lockfile that Cargo just wrote, rather than shelling out to
/// `cargo metadata`, because the lockfile is the artifact that records whether a
/// patch was actually used: a patched crate appears with no `source`.
fn parse_lock_packages(contents: &str) -> Result<Vec<LockPackage>> {
    let lock = contents
        .parse::<DocumentMut>()
        .context("failed to parse Cargo.lock")?;
    let packages = lock
        .get("package")
        .context("Cargo.lock has no `[[package]]` entries")?
        .as_array_of_tables()
        .context("`package` in Cargo.lock is not an array of tables")?;
    let mut result = Vec::new();
    for package in packages.iter() {
        let name = package
            .get("name")
            .and_then(|name| name.as_str())
            .context("a `[[package]]` entry in Cargo.lock has no `name`")?
            .to_string();
        let version = package
            .get("version")
            .and_then(|version| version.as_str())
            .with_context(|| format!("`{name}` in Cargo.lock has no `version`"))?
            .to_string();
        let source = package
            .get("source")
            .and_then(|source| source.as_str())
            .map(str::to_string);
        result.push(LockPackage {
            name,
            version,
            source,
        });
    }
    Ok(result)
}

fn validate_expected_patches(packages: &[LockPackage], expected: &[ExpectedPatch]) -> Result<()> {
    let mut problems = Vec::new();
    for patch in expected {
        let matching: Vec<&LockPackage> = packages
            .iter()
            .filter(|package| package.name == patch.name && package.version == patch.version)
            .collect();
        if !matching.iter().any(|package| package.source.is_none()) {
            let found = if matching.is_empty() {
                "it is not in the lockfile at all".to_string()
            } else {
                format!(
                    "it only appears from: {}",
                    matching
                        .iter()
                        .filter_map(|package| package.source.as_deref())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            problems.push(format!(
                "`{} {}` was patched in, but {found}",
                patch.name, patch.version
            ));
        }
        if patch.reject_any_registry_copy {
            let sources: Vec<&str> = packages
                .iter()
                .filter(|package| package.name == patch.name)
                .filter_map(|package| package.source.as_deref())
                .collect();
            if !sources.is_empty() {
                problems.push(format!(
                    "`{}` must resolve only through the patch, but registry copies remain: {}",
                    patch.name,
                    sources.join(", ")
                ));
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        bail!(
            "the compatibility transition patch set was not fully used:\n  {}",
            problems.join("\n  ")
        )
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use smithy_rs_tool_common::package::Publish;

    fn patched(crates: &[(&str, &str)]) -> BTreeMap<String, Version> {
        crates
            .iter()
            .map(|(name, version)| {
                (
                    name.to_string(),
                    Version::parse(version).expect("valid version"),
                )
            })
            .collect()
    }

    /// Rewrites `manifest` and returns the resulting document plus the scan results.
    fn rewrite(
        manifest: &str,
        patched_versions: &BTreeMap<String, Version>,
    ) -> (String, OldSdkScan) {
        let mut doc = manifest.parse::<DocumentMut>().expect("valid toml");
        let mut scan = OldSdkScan::default();
        let changed = rewrite_manifest_requirements(
            "sdk/test/Cargo.toml",
            &mut doc,
            patched_versions,
            &mut scan,
        )
        .expect("rewrite succeeds");
        assert_eq!(changed, !scan.rewrites.is_empty());
        (doc.to_string(), scan)
    }

    fn test_package(name: &str, version: &str) -> Package {
        Package::new(
            PackageHandle::new(name, Some(Version::parse(version).expect("valid version"))),
            format!("/tmp/{name}/Cargo.toml"),
            BTreeSet::new(),
            Publish::Allowed,
        )
    }

    #[test]
    fn rewrites_bare_string_requirement() {
        let (output, scan) = rewrite(
            r#"[dependencies]
# keep me
aws-smithy-json = "0.63.0" # and me
"#,
            &patched(&[("aws-smithy-json", "0.64.0")]),
        );
        assert_eq!(
            r#"[dependencies]
# keep me
aws-smithy-json = "0.64.0" # and me
"#,
            output
        );
        assert_eq!(1, scan.rewrites.len());
        assert_eq!("0.63.0", scan.rewrites[0].from);
        assert_eq!("0.64.0", scan.rewrites[0].to);
        assert_eq!("dependencies", scan.rewrites[0].table);
        assert_eq!("aws-smithy-json", scan.rewrites[0].dependency);
        assert!(scan.required.contains("aws-smithy-json"));
    }

    #[test]
    fn rewrites_inline_table_requirement_preserving_other_keys() {
        let (output, scan) = rewrite(
            r#"[dependencies]
aws-smithy-json = { version = "0.63.0", features = ["a"], default-features = false, optional = true }
"#,
            &patched(&[("aws-smithy-json", "0.64.0")]),
        );
        assert_eq!(
            r#"[dependencies]
aws-smithy-json = { version = "0.64.0", features = ["a"], default-features = false, optional = true }
"#,
            output
        );
        assert_eq!(1, scan.rewrites.len());
        // Referenced only optionally, so it is not required to be in the resolved graph.
        assert!(scan.required.is_empty());
        assert_eq!(
            vec!["aws-smithy-json"],
            scan.optional_only_names().into_iter().collect::<Vec<_>>()
        );
    }

    #[test]
    fn rewrites_standard_table_requirement() {
        let (output, scan) = rewrite(
            r#"[dependencies.aws-smithy-json]
version = "0.63.0"
features = ["a"]
"#,
            &patched(&[("aws-smithy-json", "0.64.0")]),
        );
        assert_eq!(
            r#"[dependencies.aws-smithy-json]
version = "0.64.0"
features = ["a"]
"#,
            output
        );
        assert_eq!(1, scan.rewrites.len());
        assert_eq!("dependencies", scan.rewrites[0].table);
    }

    #[test]
    fn honors_package_aliases() {
        let (output, scan) = rewrite(
            r#"[dependencies]
json-old = { package = "aws-smithy-json", version = "0.63.0" }
not-patched = { package = "serde", version = "0.63.0" }
"#,
            &patched(&[("aws-smithy-json", "0.64.0")]),
        );
        assert_eq!(
            r#"[dependencies]
json-old = { package = "aws-smithy-json", version = "0.64.0" }
not-patched = { package = "serde", version = "0.63.0" }
"#,
            output
        );
        assert_eq!(1, scan.rewrites.len());
        assert_eq!("json-old", scan.rewrites[0].dependency);
        assert_eq!("aws-smithy-json", scan.rewrites[0].package);
        assert!(scan.rewrites[0]
            .describe()
            .contains("package = \"aws-smithy-json\""));
        assert!(scan.required.contains("aws-smithy-json"));
    }

    #[test]
    fn rewrites_dev_and_build_dependencies() {
        let (output, scan) = rewrite(
            r#"[dev-dependencies]
aws-smithy-json = "0.63.0"

[build-dependencies]
aws-smithy-json = "0.63.0"
"#,
            &patched(&[("aws-smithy-json", "0.64.0")]),
        );
        assert_eq!(
            r#"[dev-dependencies]
aws-smithy-json = "0.64.0"

[build-dependencies]
aws-smithy-json = "0.64.0"
"#,
            output
        );
        assert_eq!(
            vec!["dev-dependencies", "build-dependencies"],
            scan.rewrites
                .iter()
                .map(|rewrite| rewrite.table.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn rewrites_target_specific_dependencies() {
        let (output, scan) = rewrite(
            r#"[target."cfg(unix)".dependencies]
aws-smithy-json = "0.63.0"

[target.'cfg(target_arch = "wasm32")'.dev-dependencies]
aws-smithy-json = { version = "0.63.0" }

[target."cfg(windows)".build-dependencies]
aws-smithy-json = "0.63.0"
"#,
            &patched(&[("aws-smithy-json", "0.64.0")]),
        );
        assert_eq!(
            r#"[target."cfg(unix)".dependencies]
aws-smithy-json = "0.64.0"

[target.'cfg(target_arch = "wasm32")'.dev-dependencies]
aws-smithy-json = { version = "0.64.0" }

[target."cfg(windows)".build-dependencies]
aws-smithy-json = "0.64.0"
"#,
            output
        );
        assert_eq!(3, scan.rewrites.len());
        assert!(scan
            .rewrites
            .iter()
            .all(|rewrite| rewrite.table.starts_with("target.")));
        assert!(scan
            .rewrites
            .iter()
            .any(|rewrite| rewrite.table.ends_with(".dev-dependencies")));
        assert!(scan
            .rewrites
            .iter()
            .any(|rewrite| rewrite.table.ends_with(".build-dependencies")));
    }

    #[test]
    fn leaves_compatible_and_unrelated_requirements_alone() {
        let manifest = r#"[dependencies]
# a caret requirement already accepts 0.63.1
aws-smithy-json = "0.63.0"
aws-smithy-types = { version = "1.6", features = ["a"] }
aws-smithy-runtime = ">=1.0, <2.0"
aws-smithy-xml = { path = "../aws-smithy-xml", version = "0.60.10" }
serde = "1"
"#;
        let (output, scan) = rewrite(
            manifest,
            &patched(&[
                ("aws-smithy-json", "0.63.1"),
                ("aws-smithy-types", "1.7.0"),
                ("aws-smithy-runtime", "1.14.0"),
                ("aws-smithy-xml", "0.60.10"),
            ]),
        );
        assert_eq!(manifest, output);
        assert!(scan.rewrites.is_empty());
        // Everything referenced is still recorded as required, including the path
        // dependency whose existing version requirement already accepts the patch.
        assert_eq!(
            vec![
                "aws-smithy-json",
                "aws-smithy-runtime",
                "aws-smithy-types",
                "aws-smithy-xml"
            ],
            scan.required.iter().map(String::as_str).collect::<Vec<_>>()
        );
    }

    #[test]
    fn malformed_requirement_is_an_error() {
        let mut doc = r#"[dependencies]
aws-smithy-json = "not a version req"
"#
        .parse::<DocumentMut>()
        .expect("valid toml");
        let error = rewrite_manifest_requirements(
            "sdk/test/Cargo.toml",
            &mut doc,
            &patched(&[("aws-smithy-json", "0.64.0")]),
            &mut OldSdkScan::default(),
        )
        .expect_err("a malformed requirement fails");
        let message = format!("{error:#}");
        assert!(
            message.contains("failed to parse the version requirement `not a version req`"),
            "unexpected error: {message}"
        );
        assert!(
            message.contains("`dependencies.aws-smithy-json` in sdk/test/Cargo.toml"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn patched_dependency_without_a_version_is_an_error() {
        let mut doc = r#"[dependencies]
aws-smithy-json = { workspace = true }
"#
        .parse::<DocumentMut>()
        .expect("valid toml");
        let error = rewrite_manifest_requirements(
            "sdk/test/Cargo.toml",
            &mut doc,
            &patched(&[("aws-smithy-json", "0.64.0")]),
            &mut OldSdkScan::default(),
        )
        .expect_err("a versionless patched dependency fails");
        let message = format!("{error:#}");
        assert!(
            message
                .contains("targets patched crate `aws-smithy-json` but has no version requirement"),
            "unexpected error: {message}"
        );
        assert!(
            message.contains("`dependencies.aws-smithy-json` in sdk/test/Cargo.toml"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn selects_expected_patches() {
        let crates_to_patch = vec![
            test_package("aws-smithy-json", "0.64.0"),
            test_package("aws-smithy-types", "1.7.0"),
            // Referenced only by an optional dependency entry.
            test_package("aws-smithy-mocks", "0.3.0"),
            // No old SDK dependency edge at all (the DNS/OTel case).
            test_package("aws-smithy-dns", "0.2.0"),
        ];
        let mut scan = OldSdkScan::default();
        scan.required.insert("aws-smithy-json".to_string());
        scan.required.insert("aws-smithy-types".to_string());
        scan.optional_only.insert("aws-smithy-mocks".to_string());
        let aws_config = PackageHandle::new(
            "aws-config",
            Some(Version::parse("1.12.0").expect("valid version")),
        );

        assert_eq!(
            vec![
                ExpectedPatch {
                    name: "aws-smithy-json".to_string(),
                    version: "0.64.0".to_string(),
                    reject_any_registry_copy: false,
                },
                ExpectedPatch {
                    name: "aws-smithy-types".to_string(),
                    version: "1.7.0".to_string(),
                    reject_any_registry_copy: false,
                },
                ExpectedPatch {
                    name: "aws-config".to_string(),
                    version: "1.12.0".to_string(),
                    reject_any_registry_copy: true,
                },
            ],
            select_expected_patches(&crates_to_patch, &scan, &aws_config)
        );
    }

    const LOCKFILE: &str = r#"version = 4

[[package]]
name = "aws-config"
version = "1.12.0"
dependencies = ["aws-smithy-json"]

[[package]]
name = "aws-smithy-json"
version = "0.64.0"

[[package]]
name = "aws-smithy-json"
version = "0.63.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "abc"
"#;

    #[test]
    fn lock_validation_accepts_used_patches() {
        let packages = parse_lock_packages(LOCKFILE).expect("parses");
        assert_eq!(3, packages.len());
        assert_eq!(None, packages[1].source);
        validate_expected_patches(
            &packages,
            &[
                ExpectedPatch {
                    name: "aws-smithy-json".to_string(),
                    version: "0.64.0".to_string(),
                    reject_any_registry_copy: false,
                },
                ExpectedPatch {
                    name: "aws-config".to_string(),
                    version: "1.12.0".to_string(),
                    reject_any_registry_copy: true,
                },
            ],
        )
        .expect("the patches were used");
    }

    #[test]
    fn lock_validation_rejects_missing_patch() {
        let packages = parse_lock_packages(LOCKFILE).expect("parses");
        let error = validate_expected_patches(
            &packages,
            &[ExpectedPatch {
                name: "aws-smithy-types".to_string(),
                version: "1.7.0".to_string(),
                reject_any_registry_copy: false,
            }],
        )
        .expect_err("the patch was not used");
        let message = format!("{error}");
        assert!(
            message.contains(
                "`aws-smithy-types 1.7.0` was patched in, but it is not in the lockfile at all"
            ),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn lock_validation_rejects_patch_that_only_resolves_from_a_registry() {
        let packages = parse_lock_packages(LOCKFILE).expect("parses");
        let error = validate_expected_patches(
            &packages,
            &[ExpectedPatch {
                name: "aws-smithy-json".to_string(),
                version: "0.63.0".to_string(),
                reject_any_registry_copy: false,
            }],
        )
        .expect_err("the patch was not used");
        assert!(
            format!("{error}").contains("it only appears from: registry+"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn lock_validation_rejects_registry_aws_config_at_any_version() {
        // The old SDK's workspace member is always present without a source. If
        // the patch is ignored, dependent crates can still resolve a newer
        // registry aws-config, so exclusivity must be checked by name rather
        // than only by the expected workspace-member version.
        let lockfile = r#"[[package]]
name = "aws-config"
version = "1.8.16"

[[package]]
name = "aws-config"
version = "1.12.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "abc"
"#;
        let packages = parse_lock_packages(lockfile).expect("parses");
        let error = validate_expected_patches(
            &packages,
            &[ExpectedPatch {
                name: "aws-config".to_string(),
                version: "1.8.16".to_string(),
                reject_any_registry_copy: true,
            }],
        )
        .expect_err("a registry copy remains");
        let message = format!("{error}");
        assert!(
            message.contains("`aws-config` must resolve only through the patch"),
            "unexpected error: {message}"
        );

        // The same lockfile is fine for a patch that doesn't demand exclusivity.
        validate_expected_patches(
            &packages,
            &[ExpectedPatch {
                name: "aws-config".to_string(),
                version: "1.8.16".to_string(),
                reject_any_registry_copy: false,
            }],
        )
        .expect("a non-exclusive patch tolerates the duplicate");
    }

    #[test]
    fn upserts_patch_entries_into_an_existing_patch_table() {
        let mut doc = r#"[workspace]
members = ["sdk/aws-config"]

[patch.crates-io]
some-other-crate = { path = "/keep/me" }
aws-smithy-json = { path = "/stale" }
"#
        .parse::<DocumentMut>()
        .expect("valid toml");
        upsert_patch_entries(
            &mut doc,
            &[
                ("aws-smithy-json".to_string(), "/new/json".to_string()),
                (
                    "aws-config".to_string(),
                    "/old-sdk/sdk/aws-config".to_string(),
                ),
            ],
        )
        .expect("upsert succeeds");
        assert_eq!(
            r#"[workspace]
members = ["sdk/aws-config"]

[patch.crates-io]
some-other-crate = { path = "/keep/me" }
aws-smithy-json = { path = "/new/json" }
aws-config = { path = "/old-sdk/sdk/aws-config" }
"#,
            doc.to_string()
        );
    }

    #[test]
    fn upserts_patch_entries_when_there_is_no_patch_table() {
        let mut doc = r#"[workspace]
members = ["sdk/aws-config"]
"#
        .parse::<DocumentMut>()
        .expect("valid toml");
        upsert_patch_entries(
            &mut doc,
            &[(
                "aws-config".to_string(),
                "/old-sdk/sdk/aws-config".to_string(),
            )],
        )
        .expect("upsert succeeds");
        let output = doc.to_string();
        assert!(
            output
                .contains("[patch.crates-io]\naws-config = { path = \"/old-sdk/sdk/aws-config\" }"),
            "unexpected output: {output}"
        );
        // The new table must be parseable and must not emit an empty `[patch]` header.
        assert!(!output.contains("[patch]\n"), "unexpected output: {output}");
        output.parse::<DocumentMut>().expect("still valid toml");
    }

    #[test]
    fn rewrites_old_sdk_manifests_but_skips_nested_workspaces() {
        let sdk_root = tempfile::tempdir().expect("temp dir");
        let sdk_root = sdk_root.path();
        let write = |relative: &str, contents: &str| {
            let path = sdk_root.join(relative);
            std::fs::create_dir_all(path.parent().expect("has a parent")).expect("mkdir");
            std::fs::write(&path, contents).expect("write");
            path
        };
        let client = write(
            "sdk/s3/Cargo.toml",
            r#"[dependencies]
aws-smithy-json = "0.63.0"
"#,
        );
        // Fuzz crates and similar nested workspace roots do not consume the
        // root SDK workspace's patch table, so neither they nor their children
        // may be rewritten.
        let nested_workspace = write(
            "sdk/s3/fuzz/Cargo.toml",
            r#"[workspace]
members = ["."]

[dependencies]
aws-smithy-json = { version = "0.63.0" }
"#,
        );
        let nested_workspace_child = write(
            "sdk/s3/fuzz/fixture/Cargo.toml",
            r#"[dev-dependencies]
aws-smithy-json = "0.63.0"
"#,
        );
        // Build output is not part of the workspace scan.
        let ignored = write(
            "sdk/s3/target/package/other/Cargo.toml",
            r#"[dependencies]
aws-smithy-json = "0.63.0"
"#,
        );
        // A manifest outside of `sdk/` is not touched either.
        let outside = write(
            "examples/Cargo.toml",
            r#"[dependencies]
aws-smithy-json = "0.63.0"
"#,
        );

        let scan =
            rewrite_old_sdk_requirements(sdk_root, &[test_package("aws-smithy-json", "0.64.0")])
                .expect("scan succeeds");

        assert_eq!(1, scan.rewrites.len());
        assert_eq!(
            vec!["sdk/s3/Cargo.toml"],
            scan.rewrites
                .iter()
                .map(|rewrite| rewrite.manifest.replace('\\', "/"))
                .collect::<Vec<_>>()
        );
        let read = |path: &std::path::Path| std::fs::read_to_string(path).expect("read");
        assert!(read(&client).contains(r#"aws-smithy-json = "0.64.0""#));
        assert!(read(&nested_workspace).contains(r#"{ version = "0.63.0" }"#));
        assert!(read(&nested_workspace_child).contains(r#"aws-smithy-json = "0.63.0""#));
        assert!(read(&ignored).contains(r#"aws-smithy-json = "0.63.0""#));
        assert!(read(&outside).contains(r#"aws-smithy-json = "0.63.0""#));
    }
}
