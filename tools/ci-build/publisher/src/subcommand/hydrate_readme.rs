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
        .with_context(|| format!("Failed to write hydrated README to {output:?}"))?;
    Ok(())
}

fn hydrate_template<C: Serialize>(template_string: &str, template_context: &C) -> Result<String> {
    let reg = Handlebars::new();
    reg.render_template(template_string, template_context)
        .context("Failed to hydrate README template")
}

fn make_context(msrv: &str, versions_manifest: &VersionsManifest) -> serde_json::Value {
    let mut context = json!({ "msrv": msrv });

    for (crate_name, metadata) in &versions_manifest.crates {
        let key = format!("sdk_version_{}", crate_name.replace('-', "_"));
        context
            .as_object_mut()
            .unwrap()
            .insert(key, serde_json::Value::String(metadata.version.clone()));
    }
    context
}

#[cfg(test)]
mod tests {
    use super::hydrate_template;
    use serde_json::json;

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
}
