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
reqwest = { version = "0.11.14", features = ["rustls-tls"], default-features = false }
"#;

const REQUIRED_SDK_CRATES: &[&str] = &[
    "aws-config",
    "aws-sdk-s3",
    "aws-sdk-ec2",
    "aws-sdk-transcribestreaming",
];

lazy_static! {
    static ref NOTABLE_SDK_RELEASE_TAGS: Vec<ReleaseTag> = vec![
        ReleaseTag::from_str("release-2023-01-26").unwrap(), // last version before the crate reorg
    ];
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

    /// Only generate the `Cargo.toml` file rather than building the entire bundle
    #[clap(long)]
    pub manifest_only: bool,
}

enum CrateSource {
    Path(PathBuf),
    VersionsManifest {
        versions: VersionsManifest,
        release_tag: ReleaseTag,
    },
}

fn enabled_features(crate_source: &CrateSource) -> Vec<String> {
    let mut enabled = Vec::new();
    if let CrateSource::VersionsManifest { release_tag, .. } = crate_source {
        // we want to select the newest module specified after this release
        for notable in NOTABLE_SDK_RELEASE_TAGS.iter() {
            tracing::debug!(release_tag = ?release_tag, notable = ?notable, "considering if release tag came before notable release");
            if release_tag <= notable {
                tracing::debug!("selecting {} as chosen release", notable);
                enabled.push(notable.as_str().into());
                break;
            }
        }
    }
    if enabled.is_empty() {
        enabled.push("latest".into());
    }
    enabled
}

fn generate_crate_manifest(crate_source: CrateSource) -> Result<String> {
    let mut output = BASE_MANIFEST.to_string();
    for &sdk_crate in REQUIRED_SDK_CRATES {
        match &crate_source {
            CrateSource::Path(path) => {
                let path_name = match sdk_crate.strip_prefix("aws-sdk-") {
                    Some(path) => path,
                    None => sdk_crate,
                };
                let crate_path = path.join(path_name);
                writeln!(
                    output,
                    r#"{sdk_crate} = {{ path = "{path}" }}"#,
                    path = crate_path.to_string_lossy()
                )
                .unwrap()
            }
            CrateSource::VersionsManifest {
                versions,
                release_tag,
            } => match versions.crates.get(sdk_crate) {
                Some(version) => writeln!(
                    output,
                    r#"{sdk_crate} = "{version}""#,
                    version = version.version
                )
                .unwrap(),
                None => {
                    bail!("Couldn't find `{sdk_crate}` in versions.toml for `{release_tag}`")
                }
            },
        }
    }
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
        "default = [{enabled}]",
        enabled = enabled_features(&crate_source)
            .into_iter()
            .map(|f| format!("\"{f}\""))
            .collect::<Vec<String>>()
            .join(", ")
    )
    .unwrap();
    Ok(output)
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
    let sdk_release_tag = sdk_release_tag.map(|s| s.to_string().replace('-', ""));
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
    let crate_manifest_content = generate_crate_manifest(crate_source)?;
    fs::write(&manifest_path, crate_manifest_content).context("failed to write Cargo.toml")?;

    if !opt.manifest_only {
        // Compile the canary Lambda
        let mut command = Command::new("cargo");
        command
            .arg("build")
            .arg("--release")
            .arg("--manifest-path")
            .arg(&manifest_path);
        if opt.musl {
            command.arg("--target=x86_64-unknown-linux-musl");
        }
        handle_failure("cargo build", &command.output()?)?;

        // Bundle the Lambda
        let repository_root = find_git_repository_root("smithy-rs", canary_path)?;
        let target_path = {
            let mut path = repository_root.join("tools").join("target");
            if opt.musl {
                path = path.join("x86_64-unknown-linux-musl");
            }
            path.join("release")
        };
        let bin_path = target_path.join("bootstrap");
        let bundle_path = target_path.join(name_bundle(
            &bin_path,
            opt.rust_version.as_deref(),
            opt.sdk_release_tag.as_ref(),
        )?);

        let zip_file = fs::File::create(&bundle_path).context(here!())?;
        let mut zip = zip::ZipWriter::new(zip_file);
        zip.start_file(
            "bootstrap",
            zip::write::FileOptions::default().unix_permissions(0o755),
        )
        .context(here!())?;
        zip.write_all(&fs::read(&bin_path).context(here!("read target"))?)
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
reqwest = { version = "0.11.14", features = ["rustls-tls"], default-features = false }
aws-config = { path = "some/sdk/path/aws-config" }
aws-sdk-s3 = { path = "some/sdk/path/s3" }
aws-sdk-ec2 = { path = "some/sdk/path/ec2" }
aws-sdk-transcribestreaming = { path = "some/sdk/path/transcribestreaming" }

[features]
latest = []
"release-2023-01-26" = []
default = ["latest"]
"#,
            generate_crate_manifest(CrateSource::Path("some/sdk/path".into())).expect("success")
        );
    }

    #[test]
    fn test_generate_crate_manifest_with_release_tag() {
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
reqwest = { version = "0.11.14", features = ["rustls-tls"], default-features = false }
aws-config = "0.46.0"
aws-sdk-s3 = "0.20.0"
aws-sdk-ec2 = "0.19.0"
aws-sdk-transcribestreaming = "0.16.0"

[features]
latest = []
"release-2023-01-26" = []
default = ["latest"]
"#,
            generate_crate_manifest(CrateSource::VersionsManifest {
                versions: VersionsManifest {
                    smithy_rs_revision: "some-revision-smithy-rs".into(),
                    aws_doc_sdk_examples_revision: "some-revision-docs".into(),
                    manual_interventions: Default::default(),
                    crates: [
                        crate_version("aws-config", "0.46.0"),
                        crate_version("aws-sdk-s3", "0.20.0"),
                        crate_version("aws-sdk-ec2", "0.19.0"),
                        crate_version("aws-sdk-transcribestreaming", "0.16.0"),
                    ]
                    .into_iter()
                    .collect(),
                    release: None,
                },
                release_tag: ReleaseTag::from_str("release-2023-05-26").unwrap(),
            })
            .expect("success")
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
            aws_doc_sdk_examples_revision: "some-revision-docs".into(),
            manual_interventions: Default::default(),
            crates: [].into_iter().collect(),
            release: None,
        };
        assert_eq!(
            enabled_features(&CrateSource::VersionsManifest {
                versions: versions.clone(),
                release_tag: "release-2023-02-23".parse().unwrap(),
            }),
            vec!["latest".to_string()]
        );

        assert_eq!(
            enabled_features(&CrateSource::VersionsManifest {
                versions: versions.clone(),
                release_tag: "release-2023-01-26".parse().unwrap(),
            }),
            vec!["release-2023-01-26".to_string()]
        );
        assert_eq!(
            enabled_features(&CrateSource::VersionsManifest {
                versions,
                release_tag: "release-2023-01-13".parse().unwrap(),
            }),
            vec!["release-2023-01-26".to_string()]
        );
    }
}
