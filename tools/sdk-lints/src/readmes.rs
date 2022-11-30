/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::{all_runtime_crates, anchor, Check, Fix, Lint, LintError};
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) struct ReadmesExist;
impl Lint for ReadmesExist {
    fn name(&self) -> &str {
        "Crates have readmes"
    }

    fn files_to_check(&self) -> Result<Vec<PathBuf>> {
        Ok(all_runtime_crates()?.collect())
    }
}

impl Check for ReadmesExist {
    fn check(&self, path: impl AsRef<Path>) -> Result<Vec<crate::lint::LintError>> {
        if !path.as_ref().join("README.md").exists() {
            return Ok(vec![LintError::new("Crate is missing a README")]);
        }
        Ok(vec![])
    }
}

pub(crate) struct ReadmesHaveFooters;
impl Lint for ReadmesHaveFooters {
    fn name(&self) -> &str {
        "READMEs have footers"
    }

    fn files_to_check(&self) -> Result<Vec<PathBuf>> {
        Ok(all_runtime_crates()?
            .map(|path| path.join("README.md"))
            .collect())
    }
}

impl Fix for ReadmesHaveFooters {
    fn fix(&self, path: impl AsRef<Path>) -> Result<(Vec<LintError>, String)> {
        let (updated, new_contents) = fix_readme(path)?;
        let errs = match updated {
            true => vec![LintError::new("README was missing required footer")],
            false => vec![],
        };
        Ok((errs, new_contents))
    }
}

const README_FOOTER: &str = "\nThis crate is part of the [AWS SDK for Rust](https://awslabs.github.io/aws-sdk-rust/) \
and the [smithy-rs](https://github.com/awslabs/smithy-rs) code generator. In most cases, it should not be used directly.\n";

fn fix_readme(path: impl AsRef<Path>) -> Result<(bool, String)> {
    let mut contents = fs::read_to_string(path.as_ref())
        .with_context(|| format!("failure to read readme: {:?}", path.as_ref()))?;
    let updated = anchor::replace_anchor(&mut contents, &anchor::anchors("footer"), README_FOOTER)?;
    Ok((updated, contents))
}
