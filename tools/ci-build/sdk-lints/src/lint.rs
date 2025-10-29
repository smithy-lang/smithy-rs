/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::Context;
use std::borrow::Cow;

use std::fmt::{Debug, Display, Formatter};
use std::fs::read_to_string;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct LintError {
    message: Cow<'static, str>,
    context: Option<Cow<'static, str>>,
    location: Option<PathBuf>,
}

impl LintError {
    pub(crate) fn via_display<T: Display>(t: T) -> Self {
        LintError::new(format!("{t}"))
    }
    pub(crate) fn new(message: impl Into<Cow<'static, str>>) -> Self {
        LintError {
            message: message.into(),
            context: None,
            location: None,
        }
    }
    fn at_location(self, location: PathBuf) -> Self {
        Self {
            location: Some(location),
            ..self
        }
    }
}

impl Display for LintError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(ctx) = &self.context {
            write!(f, "({ctx})")?;
        }
        if let Some(path) = &self.location {
            write!(f, "({})", path.display())?;
        }
        Ok(())
    }
}

pub(crate) trait Lint {
    fn name(&self) -> &str;
    fn files_to_check(&self) -> anyhow::Result<Vec<PathBuf>>;
    fn check_all(&self) -> anyhow::Result<Vec<LintError>>
    where
        Self: Check,
    {
        let mut errors = vec![];
        for path in self
            .files_to_check()
            .with_context(|| format!("failed to load file list for {}", self.name()))?
        {
            let new_errors = self
                .check(&path)
                .with_context(|| format!("error linting {}", &path.display()))?
                .into_iter()
                .map(|lint| lint.at_location(path.clone()));
            errors.extend(new_errors);
        }
        if errors.is_empty() {
            eprintln!("{}...OK!", self.name())
        } else {
            eprintln!("Errors for {}:", self.name());
            for error in &errors {
                eprintln!("  {error}")
            }
        }
        Ok(errors)
    }

    fn fix_all(&self, dry_run: Mode) -> anyhow::Result<Vec<PathBuf>>
    where
        Self: Fix,
    {
        let mut fixes = vec![];
        for path in self
            .files_to_check()
            .with_context(|| format!("failed to load file list for {}", self.name()))?
        {
            let (errs, new_content) = self
                .fix(&path)
                .with_context(|| format!("error attempting to fix {}", path.display()))?;
            let current_content = std::fs::read_to_string(&path).unwrap_or_default();
            if !errs.is_empty() {
                eprintln!("Errors for {}:", path.display());
                for error in &errs {
                    eprintln!("  {error}")
                }
            }
            if new_content != current_content {
                if dry_run == Mode::NoDryRun {
                    std::fs::write(&path, new_content)
                        .with_context(|| format!("failure writing fix to {}", path.display()))?;
                }
                fixes.push(path);
            }
        }
        if fixes.is_empty() {
            eprintln!("{}...OK!", self.name())
        } else {
            eprintln!("Fixed {} files for {}:", fixes.len(), self.name());
            for file in &fixes {
                eprintln!("  {}", file.display())
            }
        }
        Ok(fixes)
    }
}

pub(crate) trait Check: Lint {
    fn check(&self, path: impl AsRef<Path> + Debug) -> anyhow::Result<Vec<LintError>>;
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub(crate) enum Mode {
    DryRun,
    NoDryRun,
}

pub(crate) trait Fix: Lint {
    fn fix(&self, path: impl AsRef<Path>) -> anyhow::Result<(Vec<LintError>, String)>;
}

impl<T> Check for T
where
    T: Fix,
{
    fn check(&self, path: impl AsRef<Path>) -> anyhow::Result<Vec<LintError>> {
        let old_contents = read_to_string(path.as_ref())?;
        let (mut errs, new_contents) = self.fix(path)?;
        if new_contents != old_contents {
            errs.push(LintError::new(
                "fix would have made changes. Run `sdk-lints fix`",
            ));
        }
        Ok(errs)
    }
}
