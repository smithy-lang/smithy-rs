/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::borrow::Cow;
use std::fmt::{write, Display, Formatter};
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct LintError {
    message: Cow<'static, str>,
    context: Option<Cow<'static, str>>,
}

impl Display for LintError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, self.message)?;
        if let Some(ctx) = &self.context {
            write!("({})", ctx)?;
        }
        Ok(())
    }
}

trait Lint {
    fn files_to_check(&self) -> anyhow::Result<Vec<PathBuf>>;
    fn check(&self, path: impl AsRef<Path>) -> anyhow::Result<Vec<LintError>>;
}
