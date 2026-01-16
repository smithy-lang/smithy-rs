/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::fmt::Write as FmtWrite;
use std::fs;
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

use anyhow::{bail, Context, Result};
use clap::Parser;
use lazy_static::lazy_static;
use smithy_rs_tool_common::git::find_git_repository_root;
use smithy_rs_tool_common::here;
use smithy_rs_tool_common::release_tag::ReleaseTag;
use smithy_rs_tool_common::shell::handle_failure;
use smithy_rs_tool_common::versions_manifest::VersionsManifest;

struct RequiredDependency {
    name: &'static str,
    features: Option<Vec<&'static str>>,
    disable_default_feature: bool,
}

impl RequiredDependency {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            features: None,
            disable_default_feature: false,
        }
    }
    fn with_features(mut self, features: impl IntoIterator<Item = &'static str>) -> Self {
        self.features = Some(features.into_iter().collect());
        self
    }
    fn with_default_feature_disabled(mut self) -> Self {
        self.disable_default_feature = true;
        self
    }
    fn to_string(&self, crate_source: &CrateSource) -> Result<String> {
        match crate_source {
            CrateSource::Path(path) => {
                let path_name = match self.name.strip_prefix("aws-sdk-") {
                    Some(path) => path,
                    None => self.name,
                };
                let crate_path = path.join(path_name);
                let mut result = format!(
                    r#"{name} = {{ path = "{path}""#,
                    name = self.name,
                    path = crate_path.to_string_lossy(),
                );
                if let Some(features) = &self.features {
                    self.write_features(features, &mut result);
                }
                if self.disable_default_feature {
                    result.push_str(", default-features = false");
                }
                result.push_str(" }");
                Ok(result)
            }
            CrateSource::VersionsManifest {
                versions,
                release_tag,
            } => match versions.crates.get(self.name) {
                Some(version) => {
                    let mut result = format!(
                        r#"{name} = {{ version = "{version}""#,
                        name = self.name,
                        version = version.version,
                    );
                    if let Some(features) = &self.features {
                        self.write_features(features, &mut result);
                    }
                    if self.disable_default_feature {
                        result.push_str(", default-features = false");
                    }
                    result.push_str(" }");
                    Ok(result)
                }
                None => {
                    bail!(
                        "Couldn't find `{name}` in versions.toml for `{release_tag}`",
                        name = self.name,
                    )
                }
            },
        }
    }
    fn write_features(&self, features: &[&'static str], output: &mut String) {
        output.push_str(&format!(
            r#", features = [{features}]"#,
            features = features
                .iter()
                .map(|feature| format!(r#""{feature}""#))
                .collect::<Vec<_>>()
                .join(","),
        ));
    }
}

const BASE_MANIFEST: &str = r#"
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#
# IMPORTANT: Don't edit this file directly! Run `canary-runner build-bundle` to modify this file instead.
[package]
name = "aws-sdk-rust-lambda-canary"
version = "0.1.0"
edition = "2021"
license = "Apache-2.0"

# Emit an empty workspace so that the canary can successfully build when
# built from the aws-sdk-rust repo, which has a workspace in it.
[workspace]

[[bin]]
name = "bootstrap"
path = "src/main.rs"

[dependencies]
anyhow = "1"
async-stream = "0.3"
bytes = "1"
hound = "3.4"
async-trait = "0.1"
lambda_runtime = "0.4"
serde_json = "1"
thiserror = "1"
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["fmt", "env-filter"] }
uuid = { version = "0.8", features = ["v4"] }
tokio-stream = "0"
tracing-texray = "0.1.1"
reqwest = { version = "0.12.12", features = ["rustls-tls"], default-features = false }
edit-distance = "2"
wasmtime = "38.0.4"
wasmtime-wasi = "38.0.4"
wasmtime-wasi-http = "38.0.4"

"#;

lazy_static! {
    static ref REQUIRED_SDK_CRATES: Vec<RequiredDependency> = vec![
        RequiredDependency::new("aws-config").with_features(["behavior-version-latest"]),
        RequiredDependency::new("aws-sdk-s3").with_features(["http-1x"]),
        RequiredDependency::new("aws-sdk-ec2"),
        RequiredDependency::new("aws-sdk-transcribestreaming"),
        RequiredDependency::new("aws-smithy-wasm"),
    ];
}

const WASM_BASE_MANIFEST: &str = r#"
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#
# IMPORTANT: Don't edit this file directly! Run `canary-runner build-bundle` to modify this file instead.
[package]
name = "aws-sdk-rust-lambda-canary-wasm"
version = "0.1.0"
edition = "2021"
license = "Apache-2.0"

[lib]
crate-type = ["cdylib"]

# metadata used by cargo-component to identify which wit world to embed in the binary
[package.metadata.component]
package = "aws:component"

[dependencies]
tokio = { version = "1.36.0", features = ["macros", "rt", "time"] }
wit-bindgen = "0.51.0"
"#;

lazy_static! {
    static ref WASM_REQUIRED_SDK_CRATES: Vec<RequiredDependency> = vec![
        RequiredDependency::new("aws-config")
            .with_features(["behavior-version-latest"])
            .with_default_feature_disabled(),
        RequiredDependency::new("aws-sdk-s3").with_default_feature_disabled(),
        RequiredDependency::new("aws-smithy-async")
            .with_features(["rt-tokio"])
            .with_default_feature_disabled(),
        RequiredDependency::new("aws-smithy-wasm"),
    ];
}

// The elements in this `Vec` should be sorted in an ascending order by the release date.
lazy_static! {
    static ref NOTABLE_SDK_RELEASE_TAGS: Vec<ReleaseTag> = vec![
        // last version before addition of Sigv4a MRAP test
        ReleaseTag::from_str("release-2023-10-26").unwrap(),
    ];
}

/// Lambda architecture to target
#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
pub enum LambdaArchitecture {
    #[default]
    X86_64,
    Arm64,
}

impl std::str::FromStr for LambdaArchitecture {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "x86_64" | "x86-64" | "amd64" => Ok(LambdaArchitecture::X86_64),
            "arm64" | "aarch64" => Ok(LambdaArchitecture::Arm64),
            _ => Err(format!(
                "Unknown architecture: {s}. Use 'x86_64' or 'arm64'"
            )),
        }
    }
}

impl LambdaArchitecture {
    pub fn rust_target(&self, musl: bool) -> &'static str {
        match (self, musl) {
            (LambdaArchitecture::X86_64, false) => "x86_64-unknown-linux-gnu",
            (LambdaArchitecture::X86_64, true) => "x86_64-unknown-linux-musl",
            (LambdaArchitecture::Arm64, false) => "aarch64-unknown-linux-gnu",
            (LambdaArchitecture::Arm64, true) => "aarch64-unknown-linux-musl",
        }
    }
}

#[derive(Debug, Parser, Eq, PartialEq)]
pub struct BuildBundleArgs {
    /// Canary Lambda source code path (defaults to current directory)
    #[clap(long)]
    pub canary_path: Option<PathBuf>,

    /// Rust version
    #[clap(long)]
    pub rust_version: Option<String>,

    /// SDK release tag to use for crate version numbers
    #[clap(
        long,
        required_unless_present = "sdk-path",
        conflicts_with = "sdk-path"
    )]
    pub sdk_release_tag: Option<ReleaseTag>,

    /// SDK release tag to use for crate version numbers
    #[clap(
        long,
        required_unless_present = "sdk-release-tag",
        conflicts_with = "sdk-release-tag"
    )]
    pub sdk_path: Option<PathBuf>,

    /// Whether to target MUSL instead of GLIBC when compiling the Lambda
    #[clap(long)]
    pub musl: bool,

    /// Lambda architecture to target (x86_64 or arm64)
    #[clap(long, default_value = "x86_64")]
    pub architecture: LambdaArchitecture,

    /// Only generate the `Cargo.toml` file rather than building the entire bundle
    #[clap(long)]
    pub manifest_only: bool,
}

#[derive(Clone)]
enum CrateSource {
    Path(PathBuf),
    VersionsManifest {
        versions: VersionsManifest,
        release_tag: ReleaseTag,
    },
}

fn enabled_feature(crate_source: &CrateSource) -> String {
    if let CrateSource::VersionsManifest { release_tag, .. } = crate_source {
        // we want to select the oldest module specified after this release
        for notable in NOTABLE_SDK_RELEASE_TAGS.iter() {
            tracing::debug!(release_tag = ?release_tag, notable = ?notable, "considering if release tag came before notable release");
            if release_tag <= notable {
                tracing::debug!("selecting {} as chosen release", notable);
                return notable.as_str().into();
            }
        }
    }
    "latest".into()
}

fn generate_crate_manifest(crate_source: CrateSource) -> Result<String> {
    let mut output = BASE_MANIFEST.to_string();
    write_dependencies(&REQUIRED_SDK_CRATES, &mut output, &crate_source)?;
    write!(output, "\n[features]\n").unwrap();
    writeln!(output, "latest = []").unwrap();
    for release_tag in NOTABLE_SDK_RELEASE_TAGS.iter() {
        writeln!(
            output,
            "\"{release_tag}\" = []",
            release_tag = release_tag.as_str()
        )
        .unwrap();
    }
    writeln!(
        output,
        "default = [\"{enabled}\"]",
        enabled = enabled_feature(&crate_source)
    )
    .unwrap();
    Ok(output)
}

fn write_dependencies(
    required_crates: &[RequiredDependency],
    output: &mut String,
    crate_source: &CrateSource,
) -> Result<()> {
    for required_crate in required_crates {
        writeln!(output, "{}", required_crate.to_string(crate_source)?).unwrap();
    }
    Ok(())
}

fn sha1_file(path: &Path) -> Result<String> {
    use sha1::{Digest, Sha1};
    let contents = fs::read(path).context(here!())?;
    let mut hasher = Sha1::new();
    hasher.update(&contents);
    Ok(hex::encode(hasher.finalize()))
}

fn name_bundle(
    bin_path: &Path,
    rust_version: Option<&str>,
    sdk_release_tag: Option<&ReleaseTag>,
) -> Result<String> {
    name_hashed_bundle(&sha1_file(bin_path)?, rust_version, sdk_release_tag)
}

fn name_hashed_bundle(
    bin_hash: &str,
    rust_version: Option<&str>,
    sdk_release_tag: Option<&ReleaseTag>,
) -> Result<String> {
    // The Lambda name must be less than 64 characters, so truncate the hash a bit
    let bin_hash = &bin_hash[..24];
    // Lambda function names can't have periods in them
    let rust_version = rust_version.map(|s| s.replace('.', ""));
    let rust_version = rust_version.as_deref().unwrap_or("unknown");
    let sdk_release_tag = sdk_release_tag.map(|s| s.to_string().replace(['-', '.'], ""));
    let sdk_release_tag = sdk_release_tag.as_deref().unwrap_or("untagged");
    Ok(format!(
        "canary-{sdk_release_tag}-{rust_version}-{bin_hash}.zip"
    ))
}

pub async fn build_bundle(opt: BuildBundleArgs) -> Result<Option<PathBuf>> {
    let canary_path = opt
        .canary_path
        .unwrap_or_else(|| std::env::current_dir().expect("current dir"));

    // Determine the SDK crate source from CLI args
    let crate_source = match (opt.sdk_path, &opt.sdk_release_tag) {
        (Some(sdk_path), None) => CrateSource::Path(sdk_path),
        (None, Some(release_tag)) => CrateSource::VersionsManifest {
            versions: VersionsManifest::from_github_tag(release_tag).await?,
            release_tag: release_tag.clone(),
        },
        _ => unreachable!("clap should have validated against this"),
    };

    // Generate the canary Lambda's Cargo.toml
    let manifest_path = canary_path.join("Cargo.toml");
    let crate_manifest_content = generate_crate_manifest(crate_source.clone())?;
    fs::write(&manifest_path, crate_manifest_content)
        .context(format!("failed to write Cargo.toml in {manifest_path:?}"))?;

    // Generate the canary-wasm's Cargo.toml
    let wasm_manifest_path = canary_path.join("../canary-wasm/Cargo.toml");
    let mut wasm_crate_manifest_content = WASM_BASE_MANIFEST.to_string();
    write_dependencies(
        &WASM_REQUIRED_SDK_CRATES,
        &mut wasm_crate_manifest_content,
        &crate_source,
    )?;
    fs::write(&wasm_manifest_path, wasm_crate_manifest_content).context(format!(
        "failed to write Cargo.toml in {wasm_manifest_path:?}"
    ))?;

    let wasm_manifest_path = std::env::current_dir()
        .expect("Current dir")
        .join("../canary-wasm/Cargo.toml");

    if !opt.manifest_only {
        // Compile the canary Lambda
        let target = opt.architecture.rust_target(opt.musl);
        let mut command = Command::new("cargo");
        command
            .arg("build")
            .arg("--release")
            .arg("--manifest-path")
            .arg(&manifest_path)
            .arg(format!("--target={target}"));
        handle_failure("cargo build", &command.output()?)?;

        // Compile the wasm canary to a .wasm binary
        let mut wasm_command = Command::new("cargo");
        wasm_command
            .arg("build")
            .arg("--release")
            .arg("--target")
            .arg("wasm32-wasip2")
            .arg("--manifest-path")
            .arg(&wasm_manifest_path);
        handle_failure("cargo build (WASM bin)", &wasm_command.output()?)?;

        // Bundle the Lambda
        let repository_root = find_git_repository_root("smithy-rs", canary_path)?;
        let target = opt.architecture.rust_target(opt.musl);
        let target_path = repository_root
            .join("tools")
            .join("target")
            .join(target)
            .join("release");
        let wasm_bin_path = {
            repository_root
                .join("tools")
                .join("target")
                .join("wasm32-wasip2")
                .join("release")
                .join("aws_sdk_rust_lambda_canary_wasm.wasm")
        };
        let bin_path = target_path.join("bootstrap");
        let bundle_path = target_path.join(name_bundle(
            &bin_path,
            opt.rust_version.as_deref(),
            opt.sdk_release_tag.as_ref(),
        )?);

        tracing::debug!(wasm_bin_path = ?wasm_bin_path, bundle_path = ?bundle_path);

        let zip_file = fs::File::create(&bundle_path).context(here!())?;
        let mut zip = zip::ZipWriter::new(zip_file);
        //Write the canary bin to the zip
        zip.start_file(
            "bootstrap",
            zip::write::FileOptions::default().unix_permissions(0o755),
        )
        .context(here!())?;
        zip.write_all(&fs::read(&bin_path).context(here!("read target"))?)
            .context(here!())?;

        // Write the wasm bin to the zip
        zip.start_file(
            "aws_sdk_rust_lambda_canary_wasm.wasm",
            zip::write::FileOptions::default().unix_permissions(0o644),
        )
        .context(here!())?;
        zip.write_all(&fs::read(wasm_bin_path).context(here!("read wasm bin"))?)
            .context(here!())?;
        zip.finish().context(here!())?;

        println!(
            "{}",
            bundle_path
                .to_str()
                .expect("path is valid utf-8 for this use-case")
        );
        return Ok(Some(bundle_path));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use smithy_rs_tool_common::package::PackageCategory;
    use smithy_rs_tool_common::versions_manifest::CrateVersion;

    use crate::Args;

    use super::*;

    fn crate_version(name: &str, version: &str) -> (String, CrateVersion) {
        (
            name.to_string(),
            CrateVersion {
                category: PackageCategory::AwsRuntime, // doesn't matter for this test
                version: version.into(),
                source_hash: "some-hash".into(),
                model_hash: None,
            },
        )
    }

    #[test]
    fn test_arg_parsing() {
        assert!(Args::try_parse_from(["./canary-runner", "build-bundle"]).is_err());
        assert!(
            Args::try_parse_from(["./canary-runner", "build-bundle", "--sdk-release-tag"]).is_err()
        );
        assert!(Args::try_parse_from(["./canary-runner", "build-bundle", "--sdk-path"]).is_err());
        assert!(Args::try_parse_from(["./canary-runner", "build-bundle", "--musl"]).is_err());
        assert!(Args::try_parse_from([
            "./canary-runner",
            "build-bundle",
            "--sdk-release-tag",
            "release-2022-07-26",
            "--sdk-path",
            "some-sdk-path"
        ])
        .is_err());
        assert_eq!(
            Args::BuildBundle(BuildBundleArgs {
                canary_path: None,
                rust_version: None,
                sdk_release_tag: Some(ReleaseTag::from_str("release-2022-07-26").unwrap()),
                sdk_path: None,
                musl: false,
                architecture: LambdaArchitecture::X86_64,
                manifest_only: false,
            }),
            Args::try_parse_from([
                "./canary-runner",
                "build-bundle",
                "--sdk-release-tag",
                "release-2022-07-26"
            ])
            .expect("valid args")
        );
        assert_eq!(
            Args::BuildBundle(BuildBundleArgs {
                canary_path: Some("some-canary-path".into()),
                rust_version: None,
                sdk_release_tag: None,
                sdk_path: Some("some-sdk-path".into()),
                musl: false,
                architecture: LambdaArchitecture::X86_64,
                manifest_only: false,
            }),
            Args::try_parse_from([
                "./canary-runner",
                "build-bundle",
                "--sdk-path",
                "some-sdk-path",
                "--canary-path",
                "some-canary-path"
            ])
            .expect("valid args")
        );
        assert_eq!(
            Args::BuildBundle(BuildBundleArgs {
                canary_path: None,
                rust_version: None,
                sdk_release_tag: Some(ReleaseTag::from_str("release-2022-07-26").unwrap()),
                sdk_path: None,
                musl: true,
                architecture: LambdaArchitecture::X86_64,
                manifest_only: true,
            }),
            Args::try_parse_from([
                "./canary-runner",
                "build-bundle",
                "--sdk-release-tag",
                "release-2022-07-26",
                "--musl",
                "--manifest-only"
            ])
            .expect("valid args")
        );
        assert_eq!(
            Args::BuildBundle(BuildBundleArgs {
                canary_path: Some("some-canary-path".into()),
                rust_version: Some("stable".into()),
                sdk_release_tag: None,
                sdk_path: Some("some-sdk-path".into()),
                musl: false,
                architecture: LambdaArchitecture::X86_64,
                manifest_only: false,
            }),
            Args::try_parse_from([
                "./canary-runner",
                "build-bundle",
                "--rust-version",
                "stable",
                "--sdk-path",
                "some-sdk-path",
                "--canary-path",
                "some-canary-path"
            ])
            .expect("valid args")
        );
    }

    #[test]
    fn test_generate_crate_manifest_with_paths() {
        pretty_assertions::assert_str_eq!(
            r#"
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#
# IMPORTANT: Don't edit this file directly! Run `canary-runner build-bundle` to modify this file instead.
[package]
name = "aws-sdk-rust-lambda-canary"
version = "0.1.0"
edition = "2021"
license = "Apache-2.0"

# Emit an empty workspace so that the canary can successfully build when
# built from the aws-sdk-rust repo, which has a workspace in it.
[workspace]

[[bin]]
name = "bootstrap"
path = "src/main.rs"

[dependencies]
anyhow = "1"
async-stream = "0.3"
bytes = "1"
hound = "3.4"
async-trait = "0.1"
lambda_runtime = "0.4"
serde_json = "1"
thiserror = "1"
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["fmt", "env-filter"] }
uuid = { version = "0.8", features = ["v4"] }
tokio-stream = "0"
tracing-texray = "0.1.1"
reqwest = { version = "0.12.12", features = ["rustls-tls"], default-features = false }
edit-distance = "2"
wasmtime = "38.0.4"
wasmtime-wasi = "38.0.4"
wasmtime-wasi-http = "38.0.4"

aws-config = { path = "some/sdk/path/aws-config", features = ["behavior-version-latest"] }
aws-sdk-s3 = { path = "some/sdk/path/s3", features = ["http-1x"] }
aws-sdk-ec2 = { path = "some/sdk/path/ec2" }
aws-sdk-transcribestreaming = { path = "some/sdk/path/transcribestreaming" }
aws-smithy-wasm = { path = "some/sdk/path/aws-smithy-wasm" }

[features]
latest = []
"release-2023-10-26" = []
default = ["latest"]
"#,
            generate_crate_manifest(CrateSource::Path("some/sdk/path".into())).expect("success")
        );
    }

    #[test]
    fn test_generate_crate_manifest_with_release_tag() {
        pretty_assertions::assert_str_eq!(
            r#"
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#
# IMPORTANT: Don't edit this file directly! Run `canary-runner build-bundle` to modify this file instead.
[package]
name = "aws-sdk-rust-lambda-canary"
version = "0.1.0"
edition = "2021"
license = "Apache-2.0"

# Emit an empty workspace so that the canary can successfully build when
# built from the aws-sdk-rust repo, which has a workspace in it.
[workspace]

[[bin]]
name = "bootstrap"
path = "src/main.rs"

[dependencies]
anyhow = "1"
async-stream = "0.3"
bytes = "1"
hound = "3.4"
async-trait = "0.1"
lambda_runtime = "0.4"
serde_json = "1"
thiserror = "1"
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["fmt", "env-filter"] }
uuid = { version = "0.8", features = ["v4"] }
tokio-stream = "0"
tracing-texray = "0.1.1"
reqwest = { version = "0.12.12", features = ["rustls-tls"], default-features = false }
edit-distance = "2"
wasmtime = "38.0.4"
wasmtime-wasi = "38.0.4"
wasmtime-wasi-http = "38.0.4"

aws-config = { version = "0.46.0", features = ["behavior-version-latest"] }
aws-sdk-s3 = { version = "0.20.0", features = ["http-1x"] }
aws-sdk-ec2 = { version = "0.19.0" }
aws-sdk-transcribestreaming = { version = "0.16.0" }
aws-smithy-wasm = { version = "0.1.0" }

[features]
latest = []
"release-2023-10-26" = []
default = ["latest"]
"#,
            generate_crate_manifest(CrateSource::VersionsManifest {
                versions: VersionsManifest {
                    smithy_rs_revision: "some-revision-smithy-rs".into(),
                    aws_doc_sdk_examples_revision: None,
                    manual_interventions: Default::default(),
                    crates: [
                        crate_version("aws-config", "0.46.0"),
                        crate_version("aws-sdk-s3", "0.20.0"),
                        crate_version("aws-sdk-ec2", "0.19.0"),
                        crate_version("aws-sdk-transcribestreaming", "0.16.0"),
                        crate_version("aws-smithy-wasm", "0.1.0"),
                    ]
                    .into_iter()
                    .collect(),
                    release: None,
                },
                release_tag: ReleaseTag::from_str("release-9999-12-31").unwrap(),
            })
            .expect("success")
        );
    }

    #[test]
    fn test_generate_canary_wasm_crate_manifest_with_paths() {
        let mut output = WASM_BASE_MANIFEST.to_string();
        let crate_source = CrateSource::Path("some/sdk/path".into());
        write_dependencies(&WASM_REQUIRED_SDK_CRATES, &mut output, &crate_source).expect("success");

        pretty_assertions::assert_str_eq!(
            r#"
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#
# IMPORTANT: Don't edit this file directly! Run `canary-runner build-bundle` to modify this file instead.
[package]
name = "aws-sdk-rust-lambda-canary-wasm"
version = "0.1.0"
edition = "2021"
license = "Apache-2.0"

[lib]
crate-type = ["cdylib"]

# metadata used by cargo-component to identify which wit world to embed in the binary
[package.metadata.component]
package = "aws:component"

[dependencies]
tokio = { version = "1.36.0", features = ["macros", "rt", "time"] }
wit-bindgen = "0.51.0"
aws-config = { path = "some/sdk/path/aws-config", features = ["behavior-version-latest"], default-features = false }
aws-sdk-s3 = { path = "some/sdk/path/s3", default-features = false }
aws-smithy-async = { path = "some/sdk/path/aws-smithy-async", features = ["rt-tokio"], default-features = false }
aws-smithy-wasm = { path = "some/sdk/path/aws-smithy-wasm" }
"#,
            output,
        );
    }

    #[test]
    fn test_generate_canary_wasm_crate_manifest_with_release_tag() {
        let mut output = WASM_BASE_MANIFEST.to_string();
        let crate_source = CrateSource::VersionsManifest {
            versions: VersionsManifest {
                smithy_rs_revision: "some-revision-smithy-rs".into(),
                aws_doc_sdk_examples_revision: None,
                manual_interventions: Default::default(),
                crates: [
                    crate_version("aws-config", "0.46.0"),
                    crate_version("aws-sdk-s3", "0.20.0"),
                    crate_version("aws-smithy-async", "0.46.0"),
                    crate_version("aws-smithy-wasm", "0.1.0"),
                ]
                .into_iter()
                .collect(),
                release: None,
            },
            release_tag: ReleaseTag::from_str("release-9999-12-31").unwrap(),
        };
        write_dependencies(&WASM_REQUIRED_SDK_CRATES, &mut output, &crate_source).expect("success");

        pretty_assertions::assert_str_eq!(
            r#"
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#
# IMPORTANT: Don't edit this file directly! Run `canary-runner build-bundle` to modify this file instead.
[package]
name = "aws-sdk-rust-lambda-canary-wasm"
version = "0.1.0"
edition = "2021"
license = "Apache-2.0"

[lib]
crate-type = ["cdylib"]

# metadata used by cargo-component to identify which wit world to embed in the binary
[package.metadata.component]
package = "aws:component"

[dependencies]
tokio = { version = "1.36.0", features = ["macros", "rt", "time"] }
wit-bindgen = "0.51.0"
aws-config = { version = "0.46.0", features = ["behavior-version-latest"], default-features = false }
aws-sdk-s3 = { version = "0.20.0", default-features = false }
aws-smithy-async = { version = "0.46.0", features = ["rt-tokio"], default-features = false }
aws-smithy-wasm = { version = "0.1.0" }
"#,
            output
        );
    }

    #[test]
    fn test_name_hashed_bundle() {
        fn check(expected: &str, actual: &str) {
            assert!(
                expected.len() < 64,
                "Lambda function name must be less than 64 characters"
            );
            assert_eq!(expected, actual);
        }
        check(
            "canary-release20221216-1621-7ae6085d2105d5d1e13b10f8.zip",
            &name_hashed_bundle(
                "7ae6085d2105d5d1e13b10f882c6cb072ff5bbf8",
                Some("1.62.1"),
                Some(&ReleaseTag::from_str("release-2022-12-16").unwrap()),
            )
            .unwrap(),
        );
        check(
            "canary-release202212162-1621-7ae6085d2105d5d1e13b10f8.zip",
            &name_hashed_bundle(
                "7ae6085d2105d5d1e13b10f882c6cb072ff5bbf8",
                Some("1.62.1"),
                Some(&ReleaseTag::from_str("release-2022-12-16.2").unwrap()),
            )
            .unwrap(),
        );
        check(
            "canary-untagged-1621-7ae6085d2105d5d1e13b10f8.zip",
            &name_hashed_bundle(
                "7ae6085d2105d5d1e13b10f882c6cb072ff5bbf8",
                Some("1.62.1"),
                None,
            )
            .unwrap(),
        );
        check(
            "canary-release20221216-unknown-7ae6085d2105d5d1e13b10f8.zip",
            &name_hashed_bundle(
                "7ae6085d2105d5d1e13b10f882c6cb072ff5bbf8",
                None,
                Some(&ReleaseTag::from_str("release-2022-12-16").unwrap()),
            )
            .unwrap(),
        );
    }

    #[test]
    fn test_notable_versions() {
        let versions = VersionsManifest {
            smithy_rs_revision: "some-revision-smithy-rs".into(),
            aws_doc_sdk_examples_revision: None,
            manual_interventions: Default::default(),
            crates: [].into_iter().collect(),
            release: None,
        };
        assert_eq!(
            "latest".to_string(),
            enabled_feature(&CrateSource::VersionsManifest {
                versions: versions.clone(),
                release_tag: "release-9999-12-31".parse().unwrap(),
            }),
        );
        assert_eq!(
            "release-2023-10-26".to_string(),
            enabled_feature(&CrateSource::VersionsManifest {
                versions: versions.clone(),
                release_tag: "release-2023-10-26".parse().unwrap(),
            }),
        );
        assert_eq!(
            "release-2023-10-26".to_string(),
            enabled_feature(&CrateSource::VersionsManifest {
                versions,
                release_tag: "release-2023-01-13".parse().unwrap(),
            }),
        );
    }
}
