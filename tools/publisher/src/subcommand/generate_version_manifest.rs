/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::fs::Fs;
use crate::package::{discover_package_manifests, read_packages};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use smithy_rs_tool_common::git::GetLastCommit;
use smithy_rs_tool_common::package::PackageCategory;
use smithy_rs_tool_common::shell::{self, ShellOperation};
use std::collections::BTreeMap;
use std::fs::File;
use std::path::Path;
use tracing::info;

pub async fn subcommand_generate_version_manifest(
    smithy_build_path: &Path,
    examples_revision: &str,
    location: &Path,
) -> Result<()> {
    verify_crate_hasher_available()?;

    let smithy_rs_revision = GetLastCommit::new(std::env::current_dir()?)
        .spawn()
        .await
        .context("get smithy-rs revision")?;
    info!("Resolved smithy-rs revision to {}", smithy_rs_revision);

    let smithy_build_root = SmithyBuildRoot::from_file(smithy_build_path)?;
    let manifests = discover_package_manifests(location.into())
        .await
        .context("discover package manifests")?;
    let packages = read_packages(Fs::Real, manifests)
        .await
        .context("read packages")?;

    let mut crates = BTreeMap::new();
    for package in packages {
        // Don't include examples
        if let PackageCategory::Unknown = package.category {
            continue;
        }

        let mut model_hash = None;
        if package.handle.name.starts_with("aws-sdk-") {
            if let Some(projection) = smithy_build_root
                .projections
                .get(&package.handle.name["aws-sdk-".len()..])
            {
                model_hash = Some(hash_model(projection)?);
            }
        }
        assert!(
            matches!(package.category, PackageCategory::AwsSdk) == model_hash.is_some(),
            "all generated SDK crates should have a model hash"
        );
        crates.insert(
            package.handle.name,
            CrateVersion {
                category: package.category,
                version: package.handle.version.to_string(),
                source_hash: hash_crate(&package.crate_path).context("hash crate")?,
                model_hash,
            },
        );
    }
    info!("Discovered and hashed {} crates", crates.len());
    let versions_manifest = VersionsManifest {
        smithy_rs_revision,
        aws_doc_sdk_examples_revision: examples_revision.to_string(),
        crates,
    };
    let output = toml::to_string_pretty(&versions_manifest).context("serialize versions.toml")?;
    let manifest_file_name = location.join("versions.toml");
    info!("Writing {:?}...", manifest_file_name);
    std::fs::write(&manifest_file_name, output).context("write versions.toml")?;
    Ok(())
}

#[derive(Debug, Serialize)]
struct VersionsManifest {
    smithy_rs_revision: String,
    aws_doc_sdk_examples_revision: String,
    crates: BTreeMap<String, CrateVersion>,
}

#[derive(Debug, Serialize)]
struct CrateVersion {
    category: PackageCategory,
    version: String,
    source_hash: String,
    model_hash: Option<String>,
}

fn verify_crate_hasher_available() -> Result<()> {
    match std::process::Command::new("crate-hasher")
        .arg("--version")
        .spawn()
    {
        Ok(_) => Ok(()),
        Err(_) => {
            bail!(
                "This subcommand requires the `crate-hasher` tool to be on the PATH\n\
                Install it by going into tools/crate-hasher and running `cargo install --path .`."
            );
        }
    }
}

fn hash_crate(path: &Path) -> Result<String> {
    let output = std::process::Command::new("crate-hasher")
        .arg(path.to_str().expect("nothing special about these paths"))
        .output()?;
    shell::handle_failure("run crate-hasher", &output)?;
    let (stdout, _stderr) = shell::output_text(&output);
    Ok(stdout.trim().into())
}

fn hash_model(projection: &SmithyBuildProjection) -> Result<String> {
    let mut hashes = String::new();
    for import in &projection.imports {
        hashes.push_str(&sha256::digest_file(import).context("hash model")?);
        hashes.push('\n');
    }
    Ok(sha256::digest(hashes))
}

#[derive(Debug, Deserialize)]
struct SmithyBuildRoot {
    projections: BTreeMap<String, SmithyBuildProjection>,
}

impl SmithyBuildRoot {
    fn from_file(path: &Path) -> Result<SmithyBuildRoot> {
        serde_json::from_reader(File::open(path).context("open smithy-build.json")?)
            .context("deserialize smithy-build.json")
    }
}

#[derive(Debug, Deserialize)]
struct SmithyBuildProjection {
    imports: Vec<String>,
}
