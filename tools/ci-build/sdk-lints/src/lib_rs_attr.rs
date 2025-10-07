/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::path::{Path, PathBuf};

use crate::all_runtime_crates;
use crate::anchor::replace_anchor;
use crate::lint::{Fix, Lint, LintError};

/// Manages a standard set of `#![crate_level]` attributes
pub(super) struct StandardizedRuntimeCrateLibRsAttributes;

impl Lint for StandardizedRuntimeCrateLibRsAttributes {
    fn name(&self) -> &str {
        "Checking for the correct docs.rs attributes"
    }

    fn files_to_check(&self) -> anyhow::Result<Vec<PathBuf>> {
        Ok(all_runtime_crates()?
            .map(|crte| crte.join("src/lib.rs"))
            .collect())
    }
}

const DOCS_CFG: &str = "#![cfg_attr(docsrs, feature(doc_cfg))]";

fn check_for_auto_cfg(path: impl AsRef<Path>) -> anyhow::Result<Vec<LintError>> {
    let contents = std::fs::read_to_string(path)?;
    if !contents.contains(DOCS_CFG) {
        return Ok(vec![LintError::new("missing docsrs header")]);
    }
    Ok(vec![])
}

impl Fix for StandardizedRuntimeCrateLibRsAttributes {
    fn fix(&self, path: impl AsRef<Path>) -> anyhow::Result<(Vec<LintError>, String)> {
        let contents = std::fs::read_to_string(&path)?;
        // ensure there is only one in the crate
        let mut contents = contents.replace(DOCS_CFG, "");
        let anchor_start = "/* Automatically managed default lints */\n";
        let anchor_end = "\n/* End of automatically managed default lints */";
        // Find the end of the license header
        if let Some(pos) = contents.find("*/") {
            let newline = contents[pos..]
                .find('\n')
                .ok_or(anyhow::Error::msg("couldn't find a newline"))?
                + 1
                + pos;
            replace_anchor(
                &mut contents,
                &(anchor_start, anchor_end),
                DOCS_CFG,
                Some(newline),
            )?;
        };
        check_for_auto_cfg(path).map(|errs| (errs, contents))
    }
}
