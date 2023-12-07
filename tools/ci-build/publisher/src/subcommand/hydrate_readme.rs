/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::{Context, Result};
use clap::Parser;
use fs_err as fs;
use handlebars::Handlebars;
use serde::Serialize;
use serde_json::json;
use smithy_rs_tool_common::package::PackageCategory;
use smithy_rs_tool_common::versions_manifest::VersionsManifest;
use std::path::PathBuf;

#[derive(Parser, Debug)]
pub struct HydrateReadmeArgs {
    /// Path to the versions.toml file for this release
    #[clap(long)]
    pub versions_manifest: PathBuf,
    /// Rust MSRV to put in the README
    #[clap(long)]
    pub msrv: String,
    /// Path to the readme template to hydrate
    #[clap(short, long)]
    pub input: PathBuf,
    /// Path to output the hydrated readme into
    #[clap(short, long)]
    pub output: PathBuf,
}

pub fn subcommand_hydrate_readme(
    HydrateReadmeArgs {
        versions_manifest,
        msrv,
        input,
        output,
    }: &HydrateReadmeArgs,
) -> Result<()> {
    let versions_manifest = VersionsManifest::from_file(versions_manifest)
        .with_context(|| format!("Failed to read versions manifest at {versions_manifest:?}"))?;
    let template = fs::read_to_string(input)
        .with_context(|| format!("Failed to read README template file at {input:?}"))?;

    let context = make_context(msrv, &versions_manifest);
    let hydrated = hydrate_template(&template, &context)?;
    fs::write(output, hydrated.as_bytes())
        .with_context(|| format!("Failed to write hydrated README to {:?}", output))?;
    Ok(())
}

fn hydrate_template<C: Serialize>(template_string: &str, template_context: &C) -> Result<String> {
    let reg = Handlebars::new();
    reg.render_template(template_string, template_context)
        .context("Failed to hydrate README template")
}

fn make_context(msrv: &str, versions_manifest: &VersionsManifest) -> serde_json::Value {
    let mut context = json!({ "msrv": msrv });

    // TODO(PostGA): Remove warning banner conditionals
    context.as_object_mut().unwrap().insert(
        "warning_banner".into(),
        serde_json::Value::Bool(!stable_release(versions_manifest)),
    );

    for (crate_name, metadata) in &versions_manifest.crates {
        let key = format!("sdk_version_{}", crate_name.replace('-', "_"));
        context
            .as_object_mut()
            .unwrap()
            .insert(key, serde_json::Value::String(metadata.version.clone()));
    }
    context
}

// TODO(PostGA): Remove warning banner conditionals
fn stable_release(manifest: &VersionsManifest) -> bool {
    manifest.crates.iter().any(|(_name, version)| {
        version.category == PackageCategory::AwsSdk && !version.version.starts_with("0.")
    })
}

#[cfg(test)]
mod tests {
    use super::hydrate_template;
    use crate::subcommand::hydrate_readme::make_context;
    use serde_json::json;
    use smithy_rs_tool_common::package::PackageCategory;
    use smithy_rs_tool_common::versions_manifest::{
        CrateVersion, ManualInterventions, VersionsManifest,
    };
    use std::collections::BTreeMap;

    #[test]
    fn test_hydrate_template() {
        let template_context = json!({
            "foo": "foo value",
            "baz": "some baz value"
        });
        let hydrated = hydrate_template(
            "\
            {{!-- Not included --}}\n\
            <!-- Included -->\n\
            Some {{foo}} and {{baz}} here.\n\
            ",
            &template_context,
        )
        .unwrap();
        assert_eq!(
            "\
            <!-- Included -->\n\
            Some foo value and some baz value here.\n\
            ",
            hydrated,
        )
    }

    fn version(category: PackageCategory, version: &str) -> CrateVersion {
        CrateVersion {
            category,
            version: version.into(),
            source_hash: "dontcare".into(),
            model_hash: None,
        }
    }
    fn make_manifest(crates: &BTreeMap<String, CrateVersion>) -> VersionsManifest {
        VersionsManifest {
            smithy_rs_revision: "dontcare".into(),
            aws_doc_sdk_examples_revision: "dontcare".into(),
            manual_interventions: ManualInterventions::default(),
            crates: crates.clone(),
            release: None,
        }
    }

    // TODO(PostGA): Remove warning banner conditionals
    #[test]
    fn test_stable_release() {
        // Validate assumptions about package categories with some precondition checks
        assert_eq!(
            PackageCategory::SmithyRuntime,
            PackageCategory::from_package_name("aws-smithy-runtime"),
            "precondition"
        );
        assert_eq!(
            PackageCategory::AwsRuntime,
            PackageCategory::from_package_name("aws-runtime"),
            "precondition"
        );
        assert_eq!(
            PackageCategory::AwsSdk,
            PackageCategory::from_package_name("aws-sdk-s3"),
            "precondition"
        );

        // With S3 at 0.36, it is not considered stable
        let mut crates = BTreeMap::new();
        crates.insert(
            "aws-smithy-http".to_string(),
            version(PackageCategory::SmithyRuntime, "0.36.0"),
        );
        crates.insert(
            "aws-smithy-runtime".to_string(),
            version(PackageCategory::SmithyRuntime, "1.0.0"),
        );
        crates.insert(
            "aws-runtime".to_string(),
            version(PackageCategory::AwsRuntime, "1.0.0"),
        );
        crates.insert(
            "aws-sdk-s3".to_string(),
            version(PackageCategory::AwsSdk, "0.36.0"),
        );
        let manifest = make_manifest(&crates);
        assert!(
            !super::stable_release(&manifest),
            "it is not stable since S3 is 0.36"
        );

        // Now with S3 at 1.0, it is considered stable
        crates.insert(
            "aws-sdk-s3".to_string(),
            version(PackageCategory::AwsSdk, "1.0.0"),
        );
        let manifest = make_manifest(&crates);
        assert!(
            super::stable_release(&manifest),
            "it is stable since S3 is 1.0"
        );
    }

    // TODO(PostGA): Remove warning banner conditionals
    #[test]
    fn test_warning_banner() {
        // First, test with unstable versions
        let mut crates = BTreeMap::new();
        crates.insert(
            "aws-smithy-runtime".to_string(),
            version(PackageCategory::SmithyRuntime, "1.0.0"),
        );
        crates.insert(
            "aws-runtime".to_string(),
            version(PackageCategory::AwsRuntime, "1.0.0"),
        );
        crates.insert(
            "aws-sdk-s3".to_string(),
            version(PackageCategory::AwsSdk, "0.36.0"),
        );
        let manifest = make_manifest(&crates);

        let context = make_context("dontcare-msrv", &manifest);
        assert!(
            context
                .as_object()
                .unwrap()
                .get("warning_banner")
                .unwrap()
                .as_bool()
                .unwrap(),
            "it should have the warning banner because it's unstable"
        );

        // Next, test with stable versions
        crates.insert(
            "aws-sdk-s3".to_string(),
            version(PackageCategory::AwsSdk, "1.0.0"),
        );
        let manifest = make_manifest(&crates);

        let context = make_context("dontcare-msrv", &manifest);
        assert!(
            !context
                .as_object()
                .unwrap()
                .get("warning_banner")
                .unwrap()
                .as_bool()
                .unwrap(),
            "it should _not_ have the warning banner because it's unstable"
        );
    }
}
