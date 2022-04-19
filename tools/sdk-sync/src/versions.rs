/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::fs::{DefaultFs, Fs};
use crate::git::CommitHash;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Eq, PartialEq)]
pub struct VersionsManifest {
    pub smithy_rs_revision: CommitHash,
    pub aws_doc_sdk_examples_revision: CommitHash,
}

#[cfg_attr(test, mockall::automock)]
pub trait Versions {
    fn load(&self, aws_sdk_rust_path: &Path) -> Result<VersionsManifest>;
}

#[derive(Debug, Default)]
pub struct DefaultVersions;

impl DefaultVersions {
    pub fn new() -> Self {
        DefaultVersions
    }

    fn load_from(&self, fs: &dyn Fs, path: &Path) -> Result<VersionsManifest> {
        #[derive(Deserialize)]
        pub struct Deser {
            smithy_rs_revision: String,
            aws_doc_sdk_examples_revision: String,
        }
        let contents = fs.read_to_string(&path.join("versions.toml"))?;
        let manifest: Deser = toml::from_str(&contents).context("parse versions.toml")?;
        Ok(VersionsManifest {
            smithy_rs_revision: CommitHash::from(manifest.smithy_rs_revision),
            aws_doc_sdk_examples_revision: CommitHash::from(manifest.aws_doc_sdk_examples_revision),
        })
    }
}

impl Versions for DefaultVersions {
    fn load(&self, aws_sdk_rust_path: &Path) -> Result<VersionsManifest> {
        self.load_from(&DefaultFs::new(), aws_sdk_rust_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::MockFs;
    use std::path::PathBuf;

    #[test]
    fn parse_versions_toml() {
        let manifest = r#"
            smithy_rs_revision = 'c5e2ce5083e6525baa8a71b45ac77f994d8516aa'
            aws_doc_sdk_examples_revision = 'd9ad3f130d88c8453af5df381fdda688435fcc30'

            [crates.aws-config]
            category = 'AwsRuntime'
            version = '0.10.1'
            source_hash = '68b3b0bc9704c2821b1b40f2a8a78f0d55068400208828b3b768dcf9272c5b6a'
        "#;
        let mut fs = MockFs::default();
        fs.expect_read_to_string()
            .withf(|p| p.to_string_lossy() == "test/aws-sdk-rust/versions.toml")
            .once()
            .returning(move |_| Ok(manifest.into()));

        let actual = DefaultVersions::new()
            .load_from(&fs, &PathBuf::from("test/aws-sdk-rust"))
            .expect("success");
        assert_eq!(
            VersionsManifest {
                smithy_rs_revision: "c5e2ce5083e6525baa8a71b45ac77f994d8516aa".into(),
                aws_doc_sdk_examples_revision: "d9ad3f130d88c8453af5df381fdda688435fcc30".into(),
            },
            actual
        )
    }
}
