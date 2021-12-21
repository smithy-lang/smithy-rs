/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::lint::LintError;
use crate::{repo_root, Check, Lint};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

const EXAMPLE_ENTRY: &str = r#"
# Example changelog entries
# [[aws-sdk-rust]]
# message = "Fix typos in module documentation for generated crates"
# references = ["smithy-rs#920"]
# meta = { "breaking" = false, "tada" = false, "bug" = false }
# author = "rcoh"
#
# [[smithy-rs]]
# message = "Fix typos in module documentation for generated crates"
# references = ["smithy-rs#920"]
# meta = { "breaking" = false, "tada" = false, "bug" = false }
# author = "rcoh"
"#;

const USE_UPDATE_CHANGELOGS: &str =
    "<!-- Do not manually edit this file, use `update-changelogs` -->";

fn maintainers() -> Vec<&'static str> {
    include_str!("../smithy-rs-maintainers.txt")
        .lines()
        .collect()
}

#[derive(Deserialize)]
struct ChangelogEntry {
    message: String,
    meta: Meta,
    author: String,
    #[serde(default)]
    references: Vec<String>,
}

impl ChangelogEntry {
    /// Write a changelog entry to [out]
    ///
    /// Example output:
    /// `- Add a feature (smithy-rs#123, @contributor)`
    fn render(&self, mut out: &mut String) {
        let mut meta = String::new();
        if self.meta.bug {
            meta.push('🐛');
        }
        if self.meta.breaking {
            meta.push('⚠');
        }
        if self.meta.tada {
            meta.push('🎉');
        }
        if !meta.is_empty() {
            meta.push(' ');
        }
        let mut references = self.references.clone();
        if !maintainers().contains(&self.author.to_ascii_lowercase().as_str()) {
            references.push(format!("@{}", self.author.to_ascii_lowercase()));
        };
        if !references.is_empty() {
            write!(meta, "({}) ", references.join(", ")).unwrap();
        }
        write!(
            &mut out,
            "- {meta}{message}",
            meta = meta,
            message = indented_message(&self.message),
        )
        .unwrap();
    }
}

fn indented_message(message: &str) -> String {
    let mut out = String::new();
    for (idx, line) in message.lines().enumerate() {
        if idx > 0 {
            out.push('\n');
            if !line.is_empty() {
                out.push_str("    ");
            }
        }
        out.push_str(line);
    }
    out
}

#[derive(Deserialize)]
struct Meta {
    bug: bool,
    breaking: bool,
    tada: bool,
}

#[derive(Deserialize)]
pub(crate) struct Changelog {
    #[serde(rename = "smithy-rs")]
    #[serde(default)]
    smithy_rs: Vec<ChangelogEntry>,
    #[serde(rename = "aws-sdk-rust")]
    #[serde(default)]
    aws_sdk_rust: Vec<ChangelogEntry>,
}

/// Ensure that there are no uncommited changes to the changelog
fn no_uncommited_changes(path: &Path) -> Result<()> {
    let unstaged = !Command::new("git")
        .arg("diff")
        .arg("--exit-code")
        .arg(path)
        .status()?
        .success();
    let staged = !Command::new("git")
        .arg("diff")
        .arg("--exit-code")
        .arg("--staged")
        .arg(path)
        .status()?
        .success();
    if unstaged || staged {
        bail!("Uncommitted changes to {}", path.display())
    }
    Ok(())
}

pub(crate) fn update_changelogs(
    changelog_next: impl AsRef<Path>,
    smithy_rs: impl AsRef<Path>,
    aws_sdk_rust: impl AsRef<Path>,
    smithy_rs_version: &str,
    aws_sdk_rust_version: &str,
    date: &str,
) -> Result<()> {
    no_uncommited_changes(changelog_next.as_ref()).context(
        "CHANGELOG.next.toml had unstaged changes. Refusing to perform changelog update.",
    )?;
    let changelog = check_changelog_next(changelog_next.as_ref()).map_err(|errs| {
        anyhow::Error::msg(format!(
            "cannot update changelogs with changelog errors: {:#?}",
            errs
        ))
    })?;
    for (entries, path, version) in [
        (changelog.smithy_rs, smithy_rs.as_ref(), smithy_rs_version),
        (
            changelog.aws_sdk_rust,
            aws_sdk_rust.as_ref(),
            aws_sdk_rust_version,
        ),
    ] {
        no_uncommited_changes(path)
            .with_context(|| format!("{} had unstaged changes", path.display()))?;
        let mut update = USE_UPDATE_CHANGELOGS.to_string();
        update.push('\n');
        update.push_str(&render(entries, version, date));
        let current = std::fs::read_to_string(path)?.replace(USE_UPDATE_CHANGELOGS, "");
        update.push('\n');
        update.push_str(&current);
        std::fs::write(path, update)?;
    }
    std::fs::write(changelog_next.as_ref(), EXAMPLE_ENTRY.trim())?;
    eprintln!("Changelogs updated!");
    Ok(())
}

/// Convert a list of changelog entries into markdown
fn render(mut entries: Vec<ChangelogEntry>, version: &str, date: &str) -> String {
    entries.sort_by_key(|ent| !ent.meta.tada);
    let mut out = String::new();
    let header = format!("{version} ({date})", version = version, date = date);
    out.push_str(&header);
    out.push('\n');
    for _ in 0..header.len() {
        out.push('=');
    }
    out.push('\n');
    let (breaking, non_breaking) = entries
        .iter()
        .partition::<Vec<_>, _>(|entry| entry.meta.breaking);

    if !breaking.is_empty() {
        out.push_str("**Breaking Changes:**\n");
        for change in breaking {
            change.render(&mut out);
            out.push_str("\n");
        }
        out.push('\n')
    }

    if !non_breaking.is_empty() {
        out.push_str("**New this release:**\n");
        for change in non_breaking {
            change.render(&mut out);
            out.push_str("\n");
        }
        out.push('\n');
    }

    let mut external_contribs = entries
        .iter()
        .map(|entry| entry.author.to_ascii_lowercase())
        .filter(|author| !maintainers().contains(&author.as_str()))
        .collect::<Vec<_>>();
    external_contribs.sort();
    external_contribs.dedup();
    if !external_contribs.is_empty() {
        out.push_str("**Contributors**\nThank you for your contributions! ❤\n");
        for contributor_handle in external_contribs {
            // retrieve all contributions this author made
            let mut contribution_references = entries
                .iter()
                .filter(|entry| {
                    entry
                        .author
                        .eq_ignore_ascii_case(contributor_handle.as_str())
                })
                .flat_map(|entry| entry.references.iter().map(|it| it.as_str()))
                .collect::<Vec<_>>();
            contribution_references.sort();
            contribution_references.dedup();
            let contribution_references = contribution_references.as_slice().join(", ");
            out.push_str("- @");
            out.push_str(&contributor_handle);
            if !contribution_references.is_empty() {
                out.push_str(&format!(" ({})", contribution_references));
            }
            out.push('\n');
        }
    }

    out
}

/// Validate a changelog entry to ensure it follows standards
fn validate(entry: &ChangelogEntry) -> Result<()> {
    if entry.author.is_empty() {
        bail!("Author must be set (was empty)");
    }
    if !entry
        .author
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-')
    {
        bail!("Author must be valid GitHub username: [a-zA-Z0-9\\-]")
    }
    if entry.references.is_empty() {
        bail!("Changelog entry must refer to at least one pull request or issue");
    }

    for reference in &entry.references {
        match reference.split_once('#') {
            None => bail!(
                "Reference must of the form `repo#number` but found {}",
                reference
            ),
            Some(("aws-sdk-rust" | "smithy-rs", number)) if number.parse::<u32>().is_ok() => {}
            _other => bail!(
                "unexpected reference format: {} (expected aws-sdk-rust/smithy-rs#number)",
                reference
            ),
        }
    }

    Ok(())
}

pub(crate) struct ChangelogNext;
impl Lint for ChangelogNext {
    fn name(&self) -> &str {
        "Changelog.next"
    }

    fn files_to_check(&self) -> Result<Vec<PathBuf>> {
        Ok(vec![repo_root().join("CHANGELOG.next.toml")])
    }
}

impl Check for ChangelogNext {
    fn check(&self, path: impl AsRef<Path>) -> Result<Vec<LintError>> {
        match check_changelog_next(path) {
            Ok(_) => Ok(vec![]),
            Err(errs) => Ok(errs),
        }
    }
}

/// Validate that `CHANGELOG.next.toml` follows best practices
fn check_changelog_next(path: impl AsRef<Path>) -> std::result::Result<Changelog, Vec<LintError>> {
    let contents = std::fs::read_to_string(path)
        .context("failed to read CHANGELOG.next")
        .map_err(|e| vec![LintError::via_display(e)])?;
    let parsed: Changelog = toml::from_str(&contents)
        .context("Invalid changelog format")
        .map_err(|e| vec![LintError::via_display(e)])?;
    let mut errors = vec![];
    for entry in parsed.aws_sdk_rust.iter().chain(parsed.smithy_rs.iter()) {
        if let Err(e) = validate(entry) {
            errors.push(LintError::via_display(e))
        }
    }
    if errors.is_empty() {
        Ok(parsed)
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod test {
    use crate::changelog::{render, Changelog};

    #[test]
    fn end_to_end_changelog() {
        let changelog_toml = r#"
[[smithy-rs]]
author = "rcoh"
message = "I made a major change to update the code generator"
meta = { breaking = true, tada = false, bug = false }
references = ["smithy-rs#445"]

[[smithy-rs]]
author = "external-contrib"
message = "I made a change to update the code generator"
meta = { breaking = false, tada = true, bug = false }
references = ["smithy-rs#446"]

[[smithy-rs]]
author = "another-contrib"
message = "I made a minor change"
meta = { breaking = false, tada = false, bug = false }

[[aws-sdk-rust]]
author = "rcoh"
message = "I made a major change to update the AWS SDK"
meta = { breaking = true, tada = false, bug = false }
references = ["smithy-rs#445"]

[[aws-sdk-rust]]
author = "external-contrib"
message = "I made a change to update the code generator"
meta = { breaking = false, tada = true, bug = false }
references = ["smithy-rs#446"]

[[smithy-rs]]
author = "external-contrib"
message = """
I made a change to update the code generator

**Update guide:**
blah blah
"""
meta = { breaking = false, tada = true, bug = false }
references = ["smithy-rs#446"]
        "#;
        let changelog: Changelog = toml::from_str(changelog_toml).expect("valid changelog");
        let rendered = render(changelog.smithy_rs, "v0.3.0", "January 4th, 2022");

        let expected = r#"
v0.3.0 (January 4th, 2022)
==========================
**Breaking Changes:**
- ⚠ (smithy-rs#445) I made a major change to update the code generator

**New this release:**
- 🎉 (smithy-rs#446, @external-contrib) I made a change to update the code generator
- 🎉 (smithy-rs#446, @external-contrib) I made a change to update the code generator

    **Update guide:**
    blah blah
- (@another-contrib) I made a minor change

**Contributors**
Thank you for your contributions! ❤
- @another-contrib
- @external-contrib (smithy-rs#446)
"#
        .trim_start();
        assert_eq!(expected, rendered);
    }
}
