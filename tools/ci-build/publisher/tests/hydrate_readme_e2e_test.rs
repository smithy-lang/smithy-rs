/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use fs_err as fs;
use publisher::subcommand::hydrate_readme::{subcommand_hydrate_readme, HydrateReadmeArgs};
use smithy_rs_tool_common::package::PackageCategory;
use smithy_rs_tool_common::versions_manifest::{CrateVersion, VersionsManifest};
use tempfile::TempDir;

#[test]
fn test_hydrate_readme() {
    let tmp_dir = TempDir::new().unwrap();

    let versions_manifest_path = tmp_dir.path().join("versions.toml");
    VersionsManifest {
        smithy_rs_revision: "dontcare".into(),
        aws_doc_sdk_examples_revision: None,
        manual_interventions: Default::default(),
        crates: [
            (
                "aws-config".to_string(),
                CrateVersion {
                    category: PackageCategory::AwsRuntime,
                    version: "0.12.3".into(),
                    source_hash: "dontcare".into(),
                    model_hash: None,
                },
            ),
            (
                "aws-sdk-dynamodb".to_string(),
                CrateVersion {
                    category: PackageCategory::AwsRuntime,
                    version: "0.14.5".into(),
                    source_hash: "dontcare".into(),
                    model_hash: None,
                },
            ),
        ]
        .into_iter()
        .collect(),
        release: None,
    }
    .write_to_file(&versions_manifest_path)
    .unwrap();

    let input_path = tmp_dir.path().join("some-input.md.hb");
    fs::write(
        &input_path,
        "{{!-- Not included --}}\n\
            <!-- Included -->\n\
            Some info about MSRV {{msrv}} here.\n\n\
            Some info about aws-sdk-dynamodb-{{sdk_version_aws_sdk_dynamodb}} here.\n\n\
            Something about aws-config-{{sdk_version_aws_config}} here.\n\n",
    )
    .unwrap();

    let output_path = tmp_dir.path().join("test-output.md");
    subcommand_hydrate_readme(&HydrateReadmeArgs {
        versions_manifest: versions_manifest_path,
        msrv: "0.58.1".into(),
        input: input_path,
        output: output_path.clone(),
    })
    .unwrap();

    let output = fs::read_to_string(&output_path).unwrap();
    pretty_assertions::assert_str_eq!(
        "<!-- Included -->\n\
        Some info about MSRV 0.58.1 here.\n\n\
        Some info about aws-sdk-dynamodb-0.14.5 here.\n\n\
        Something about aws-config-0.12.3 here.\n\n",
        output
    );
}
