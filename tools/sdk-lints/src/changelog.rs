/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::lint::LintError;
use crate::{repo_root, Check, Lint};
use anyhow::{bail, Context, Result};
use serde::{de, Deserialize, Deserializer, Serialize};
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

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

enum ChangelogEntry {
    HandAuthored(HandAuthoredEntry),
    AwsSdkModel(SdkModelEntry),
}

impl ChangelogEntry {
    fn hand_authored(&self) -> Option<&HandAuthoredEntry> {
        match self {
            ChangelogEntry::HandAuthored(hand_authored) => Some(hand_authored),
            _ => None,
        }
    }

    fn aws_sdk_model(&self) -> Option<&SdkModelEntry> {
        match self {
            ChangelogEntry::AwsSdkModel(sdk_model) => Some(sdk_model),
            _ => None,
        }
    }
}

#[derive(Deserialize)]
struct HandAuthoredEntry {
    message: String,
    meta: Meta,
    author: String,
    #[serde(default)]
    references: Vec<Reference>,
}

impl HandAuthoredEntry {
    /// Validate a changelog entry to ensure it follows standards
    fn validate(&self) -> Result<()> {
        if self.author.is_empty() {
            bail!("Author must be set (was empty)");
        }
        if !self.author.chars().all(|c| c.is_alphanumeric() || c == '-') {
            bail!("Author must be valid GitHub username: [a-zA-Z0-9\\-]")
        }
        if self.references.is_empty() {
            bail!("Changelog entry must refer to at least one pull request or issue");
        }

        Ok(())
    }

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
        let mut references = self
            .references
            .iter()
            .map(Reference::to_md_link)
            .collect::<Vec<_>>();
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

#[derive(Deserialize)]
enum SdkModelChangeKind {
    Documentation,
    Feature,
}

#[derive(Deserialize)]
struct SdkModelEntry {
    /// SDK module name (e.g., "aws-sdk-s3" for S3)
    module: String,
    /// SDK module version number (e.g., "0.14.0")
    version: String,
    /// What changed
    kind: SdkModelChangeKind,
    /// More details about the change
    message: String,
}

impl SdkModelEntry {
    fn render(&self, out: &mut String) {
        write!(
            out,
            "- `{module}` ({version}): {message}",
            module = self.module,
            version = self.version,
            message = self.message
        )
        .unwrap();
    }
}

struct Reference {
    repo: String,
    number: usize,
}

impl Reference {
    fn to_md_link(&self) -> String {
        format!(
            "[{repo}#{number}](https://github.com/awslabs/{repo}/issues/{number})",
            repo = self.repo,
            number = self.number
        )
    }
}

impl<'de> Deserialize<'de> for Reference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        FromStr::from_str(&s).map_err(de::Error::custom)
    }
}

impl FromStr for Reference {
    type Err = anyhow::Error;

    fn from_str(reference: &str) -> std::result::Result<Self, Self::Err> {
        match reference.split_once('#') {
            None => bail!(
                "Reference must of the form `repo#number` but found {}",
                reference
            ),
            Some((repo, number)) => {
                let number = number.parse::<usize>()?;
                if !matches!(repo, "smithy-rs" | "aws-sdk-rust") {
                    bail!("unexpected repo: {}", repo);
                }
                Ok(Reference {
                    number,
                    repo: repo.to_string(),
                })
            }
        }
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
    smithy_rs: Vec<HandAuthoredEntry>,
    #[serde(rename = "aws-sdk-rust")]
    #[serde(default)]
    aws_sdk_rust: Vec<HandAuthoredEntry>,
    #[serde(rename = "aws-sdk-model")]
    #[serde(default)]
    sdk_models: Vec<SdkModelEntry>,
}

impl Changelog {
    fn into_entries(mut self) -> ChangelogEntries {
        self.aws_sdk_rust.sort_by_key(|entry| !entry.meta.tada);
        self.sdk_models.sort_by(|a, b| a.module.cmp(&b.module));
        self.smithy_rs.sort_by_key(|entry| !entry.meta.tada);

        ChangelogEntries {
            smithy_rs: self
                .smithy_rs
                .into_iter()
                .map(ChangelogEntry::HandAuthored)
                .collect(),
            aws_sdk_rust: self
                .aws_sdk_rust
                .into_iter()
                .map(ChangelogEntry::HandAuthored)
                .chain(self.sdk_models.into_iter().map(ChangelogEntry::AwsSdkModel))
                .collect(),
        }
    }
}

struct ChangelogEntries {
    smithy_rs: Vec<ChangelogEntry>,
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

pub struct ReleaseMetadata {
    pub title: String,
    pub tag: String,
    pub manifest_name: String,
}

#[derive(Serialize)]
struct ReleaseManifest {
    #[serde(rename = "tagName")]
    tag_name: String,
    name: String,
    body: String,
    prerelease: bool,
}

pub(crate) fn update_changelogs(
    changelog_next: impl AsRef<Path>,
    smithy_rs_path: impl AsRef<Path>,
    aws_sdk_rust_path: impl AsRef<Path>,
    smithy_rs_metadata: &ReleaseMetadata,
    aws_sdk_rust_metadata: &ReleaseMetadata,
    release_manifest_output_path: Option<&Path>,
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
    let ChangelogEntries {
        smithy_rs,
        aws_sdk_rust,
    } = changelog.into_entries();
    for (entries, path, release_metadata) in [
        (smithy_rs, smithy_rs_path.as_ref(), smithy_rs_metadata),
        (
            aws_sdk_rust,
            aws_sdk_rust_path.as_ref(),
            aws_sdk_rust_metadata,
        ),
    ] {
        no_uncommited_changes(path)
            .with_context(|| format!("{} had unstaged changes", path.display()))?;
        let (release_header, release_notes) = render(&entries, &release_metadata.title);
        if let Some(output_path) = release_manifest_output_path {
            let release_manifest = ReleaseManifest {
                tag_name: release_metadata.tag.clone(),
                name: release_metadata.title.clone(),
                body: release_notes.clone(),
                // All releases are pre-releases for now
                prerelease: true,
            };
            std::fs::write(
                output_path.join(&release_metadata.manifest_name),
                serde_json::to_string_pretty(&release_manifest)?,
            )?;
        }

        let mut update = USE_UPDATE_CHANGELOGS.to_string();
        update.push('\n');
        update.push_str(&release_header);
        update.push_str(&release_notes);
        let current = std::fs::read_to_string(path)?.replace(USE_UPDATE_CHANGELOGS, "");
        update.push_str(&current);
        std::fs::write(path, update)?;
    }
    std::fs::write(changelog_next.as_ref(), EXAMPLE_ENTRY.trim())?;
    eprintln!("Changelogs updated!");
    Ok(())
}

fn render_handauthored<'a>(entries: impl Iterator<Item = &'a HandAuthoredEntry>, out: &mut String) {
    let (breaking, non_breaking) = entries.partition::<Vec<_>, _>(|entry| entry.meta.breaking);

    if !breaking.is_empty() {
        out.push_str("**Breaking Changes:**\n");
        for change in breaking {
            change.render(out);
            out.push('\n');
        }
        out.push('\n')
    }

    if !non_breaking.is_empty() {
        out.push_str("**New this release:**\n");
        for change in non_breaking {
            change.render(out);
            out.push('\n');
        }
        out.push('\n');
    }
}

fn render_sdk_model_entries<'a>(
    entries: impl Iterator<Item = &'a SdkModelEntry>,
    out: &mut String,
) {
    let (features, docs) =
        entries.partition::<Vec<_>, _>(|entry| matches!(entry.kind, SdkModelChangeKind::Feature));
    if !features.is_empty() {
        out.push_str("**Service Features:**\n");
        for entry in features {
            entry.render(out);
            out.push('\n');
        }
        out.push('\n');
    }
    if !docs.is_empty() {
        out.push_str("**Service Documentation:**\n");
        for entry in docs {
            entry.render(out);
            out.push('\n');
        }
        out.push('\n');
    }
}

/// Convert a list of changelog entries into markdown.
/// Returns (header, body)
fn render(entries: &[ChangelogEntry], release_header: &str) -> (String, String) {
    let mut header = String::new();
    header.push_str(release_header);
    header.push('\n');
    for _ in 0..release_header.len() {
        header.push('=');
    }
    header.push('\n');

    let mut out = String::new();
    render_handauthored(
        entries.iter().filter_map(ChangelogEntry::hand_authored),
        &mut out,
    );
    render_sdk_model_entries(
        entries.iter().filter_map(ChangelogEntry::aws_sdk_model),
        &mut out,
    );

    let mut external_contribs = entries
        .iter()
        .filter_map(|entry| entry.hand_authored().map(|e| e.author.to_ascii_lowercase()))
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
                        .hand_authored()
                        .map(|e| e.author.eq_ignore_ascii_case(contributor_handle.as_str()))
                        .unwrap_or(false)
                })
                .flat_map(|entry| {
                    entry
                        .hand_authored()
                        .unwrap()
                        .references
                        .iter()
                        .map(|it| it.to_md_link())
                })
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

    (header, out)
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
        if let Err(e) = entry.validate() {
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
    use super::ChangelogEntry;
    use crate::changelog::{render, Changelog, ChangelogEntries};

    fn render_full(entries: &[ChangelogEntry], release_header: &str) -> String {
        let (header, body) = render(entries, release_header);
        return format!("{}{}", header, body);
    }

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

[[aws-sdk-model]]
module = "aws-sdk-s3"
version = "0.14.0"
kind = "Feature"
message = "Some new API to do X"

[[aws-sdk-model]]
module = "aws-sdk-ec2"
version = "0.12.0"
kind = "Documentation"
message = "Updated some docs"

[[aws-sdk-model]]
module = "aws-sdk-ec2"
version = "0.12.0"
kind = "Feature"
message = "Some API change"
        "#;
        let changelog: Changelog = toml::from_str(changelog_toml).expect("valid changelog");
        let ChangelogEntries {
            aws_sdk_rust,
            smithy_rs,
        } = changelog.into_entries();

        let smithy_rs_rendered = render_full(&smithy_rs, "v0.3.0 (January 4th, 2022)");
        let smithy_rs_expected = r#"
v0.3.0 (January 4th, 2022)
==========================
**Breaking Changes:**
- ⚠ ([smithy-rs#445](https://github.com/awslabs/smithy-rs/issues/445)) I made a major change to update the code generator

**New this release:**
- 🎉 ([smithy-rs#446](https://github.com/awslabs/smithy-rs/issues/446), @external-contrib) I made a change to update the code generator
- 🎉 ([smithy-rs#446](https://github.com/awslabs/smithy-rs/issues/446), @external-contrib) I made a change to update the code generator

    **Update guide:**
    blah blah
- (@another-contrib) I made a minor change

**Contributors**
Thank you for your contributions! ❤
- @another-contrib
- @external-contrib ([smithy-rs#446](https://github.com/awslabs/smithy-rs/issues/446))
"#
        .trim_start();
        pretty_assertions::assert_str_eq!(smithy_rs_expected, smithy_rs_rendered);

        let aws_sdk_rust_rendered = render_full(&aws_sdk_rust, "v0.1.0 (January 4th, 2022)");
        let aws_sdk_expected = r#"
v0.1.0 (January 4th, 2022)
==========================
**Breaking Changes:**
- ⚠ ([smithy-rs#445](https://github.com/awslabs/smithy-rs/issues/445)) I made a major change to update the AWS SDK

**New this release:**
- 🎉 ([smithy-rs#446](https://github.com/awslabs/smithy-rs/issues/446), @external-contrib) I made a change to update the code generator

**Service Features:**
- `aws-sdk-ec2` (0.12.0): Some API change
- `aws-sdk-s3` (0.14.0): Some new API to do X

**Service Documentation:**
- `aws-sdk-ec2` (0.12.0): Updated some docs

**Contributors**
Thank you for your contributions! ❤
- @external-contrib ([smithy-rs#446](https://github.com/awslabs/smithy-rs/issues/446))
"#
        .trim_start();
        pretty_assertions::assert_str_eq!(aws_sdk_expected, aws_sdk_rust_rendered);
    }
}
