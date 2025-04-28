/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::{bail, Result};
use clap::Parser;
use smithy_rs_tool_common::versions_manifest::{Release, VersionsManifest};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Parser, Debug)]
pub struct TagVersionsManifestArgs {
    /// Path to the `versions.toml` file to tag
    #[clap(long)]
    manifest_path: PathBuf,
    /// Release tag to add to the `[release]` section
    #[clap(long)]
    tag: String,
}

pub fn subcommand_tag_versions_manifest(
    TagVersionsManifestArgs { manifest_path, tag }: &TagVersionsManifestArgs,
) -> Result<()> {
    println!("Tagging manifest at: {manifest_path:#?}");
    let mut manifest = VersionsManifest::from_file(manifest_path)?;
    let mut release = manifest.release.as_mut();
    if let Some(rel) = release {
        rel.tag = Some(tag.to_string());
    } else {
        release = Some(&mut Release {
            tag: Some(tag.to_string()),
            crates: BTreeMap::new(),
        });
        // bail!("The given versions manifest file doesn't have a `[release]` section in it to add the tag to");
    }
    manifest.write_to_file(manifest_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{subcommand_tag_versions_manifest, TagVersionsManifestArgs};
    use smithy_rs_tool_common::package::PackageCategory;
    use smithy_rs_tool_common::versions_manifest::{CrateVersion, Release, VersionsManifest};
    use tempfile::TempDir;

    #[test]
    fn test_tag_versions_manifest() {
        let tmp_dir = TempDir::new().unwrap();

        let versions_manifest_path = tmp_dir.path().join("versions.toml");
        let original = VersionsManifest {
            smithy_rs_revision: "some-revision-smithy-rs".into(),
            aws_doc_sdk_examples_revision: "some-revision-docs".into(),
            manual_interventions: Default::default(),
            crates: [
                (
                    "aws-config".to_string(),
                    CrateVersion {
                        category: PackageCategory::AwsRuntime,
                        version: "0.12.3".into(),
                        source_hash: "some-hash-aws-config".into(),
                        model_hash: None,
                    },
                ),
                (
                    "aws-sdk-dynamodb".to_string(),
                    CrateVersion {
                        category: PackageCategory::AwsRuntime,
                        version: "0.14.5".into(),
                        source_hash: "some-hash-aws-sdk-dynamodb".into(),
                        model_hash: None,
                    },
                ),
            ]
            .into_iter()
            .collect(),
            release: Some(Release {
                tag: None,
                crates: [("aws-config", "0.12.3"), ("aws-sdk-dynamodb", "0.14.5")]
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            }),
        };
        original.write_to_file(&versions_manifest_path).unwrap();

        subcommand_tag_versions_manifest(&TagVersionsManifestArgs {
            manifest_path: versions_manifest_path.clone(),
            tag: "some-release-tag".into(),
        })
        .unwrap();

        let expected = {
            let mut expected = original;
            expected.release.as_mut().unwrap().tag = Some("some-release-tag".to_string());
            expected
        };
        let actual = VersionsManifest::from_file(&versions_manifest_path).unwrap();
        assert_eq!(expected, actual);
    }
}
