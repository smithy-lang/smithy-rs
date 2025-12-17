/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! This module holds deserializable structs for the hand-authored changelog TOML files used in smithy-rs.

pub mod parser;

use crate::changelog::parser::{ParseIntoChangelog, ParserChain};
use anyhow::{bail, Context, Result};
use serde::{de, Deserialize, Deserializer, Serialize};
use std::collections::HashSet;
use std::fmt;
use std::fmt::Debug;
use std::path::Path;
use std::str::FromStr;

#[derive(Copy, Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub enum SdkAffected {
    #[serde(rename = "client")]
    Client,
    #[serde(rename = "server")]
    Server,
    #[serde(rename = "all")]
    #[default]
    All,
}

impl fmt::Display for SdkAffected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SdkAffected::Client => write!(f, "client"),
            SdkAffected::Server => write!(f, "server"),
            SdkAffected::All => write!(f, "all"),
        }
    }
}

impl FromStr for SdkAffected {
    type Err = anyhow::Error;

    fn from_str(sdk: &str) -> std::result::Result<Self, Self::Err> {
        match sdk.to_lowercase().as_str() {
            "client" => Ok(SdkAffected::Client),
            "server" => Ok(SdkAffected::Server),
            "all" => Ok(SdkAffected::All),
            _ => bail!("An invalid type of SDK type {sdk} has been mentioned in the meta tags"),
        }
    }
}

/// allow incase sensitive comparison of enum variants
impl<'de> Deserialize<'de> for SdkAffected {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        FromStr::from_str(&s).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Meta {
    pub bug: bool,
    pub breaking: bool,
    pub tada: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<SdkAffected>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reference {
    pub repo: String,
    pub number: usize,
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

impl Serialize for Reference {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!("{}#{}", self.repo, self.number))
    }
}

impl fmt::Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.repo, self.number)
    }
}

impl FromStr for Reference {
    type Err = anyhow::Error;

    fn from_str(reference: &str) -> std::result::Result<Self, Self::Err> {
        match reference.split_once('#') {
            None => bail!(
                "Reference must of the form `repo#number` but found {reference}"
            ),
            Some((repo, number)) => {
                let number = number.parse::<usize>()?;
                if !matches!(repo, "smithy-rs" | "aws-sdk-rust" | "aws-sdk") {
                    bail!("unexpected repo: {repo}");
                }
                Ok(Reference {
                    number,
                    repo: repo.to_string(),
                })
            }
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum AuthorsInner {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(from = "AuthorsInner", into = "AuthorsInner")]
pub struct Authors(pub(super) Vec<String>);

impl PartialEq for Authors {
    fn eq(&self, other: &Self) -> bool {
        // `true` if two `Authors` contain the same set of authors, regardless of their order
        self.0.iter().collect::<HashSet<_>>() == other.0.iter().collect::<HashSet<_>>()
    }
}

impl Eq for Authors {}

impl From<AuthorsInner> for Authors {
    fn from(value: AuthorsInner) -> Self {
        match value {
            AuthorsInner::Single(author) => Authors(vec![author]),
            AuthorsInner::Multiple(authors) => Authors(authors),
        }
    }
}

impl From<Authors> for AuthorsInner {
    fn from(mut value: Authors) -> Self {
        match value.0.len() {
            0 => Self::Single("".to_string()),
            1 => Self::Single(value.0.pop().unwrap()),
            _ => Self::Multiple(value.0),
        }
    }
}

impl Authors {
    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.0.iter()
    }

    // Checks whether the number of authors is 0 or any author has a empty name.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty() || self.iter().any(String::is_empty)
    }

    pub fn validate_usernames(&self) -> Result<()> {
        fn validate_username(author: &str) -> Result<()> {
            if !author.chars().all(|c| c.is_alphanumeric() || c == '-') {
                bail!("Author, \"{author}\", is not a valid GitHub username: [a-zA-Z0-9\\-]")
            }
            Ok(())
        }
        for author in self.iter() {
            validate_username(author)?
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct HandAuthoredEntry {
    pub message: String,
    pub meta: Meta,
    // Retain singular field named "author" for backwards compatibility,
    // but also accept plural "authors".
    #[serde(rename = "author", alias = "authors")]
    pub authors: Authors,
    #[serde(default)]
    pub references: Vec<Reference>,
    /// Optional commit hash to indicate "since when" these changes were made
    #[serde(rename = "since-commit")]
    pub since_commit: Option<String>,
    /// Optional age of this entry, for the SDK use-case where entries must be
    /// preserved across multiple smithy-rs releases. This allows the changelogger
    /// to eventually cull older entries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age: Option<usize>,
}

impl HandAuthoredEntry {
    /// Validate a changelog entry to ensure it follows standards
    pub fn validate(&self, validation_set: ValidationSet) -> Result<()> {
        if self.authors.iter().any(|author| author.trim().is_empty()) {
            bail!("Author must be set (was empty)");
        }
        if !self
            .authors
            .iter()
            .any(|author| author.chars().all(|c| c.is_alphanumeric() || c == '-'))
        {
            bail!("Author must be valid GitHub username: [a-zA-Z0-9\\-]")
        }
        if validation_set == ValidationSet::Development && self.references.is_empty() {
            bail!("Changelog entry must refer to at least one pull request or issue");
        }
        if validation_set == ValidationSet::Development && self.message.len() > 800 {
            bail!(
                "Your changelog entry is too long. Post long-form change log entries in \
                the GitHub Discussions under the Changelog category, and link to them from \
                the changelog."
            );
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Eq, Deserialize, PartialEq, Serialize)]
pub enum SdkModelChangeKind {
    Documentation,
    Feature,
}

#[derive(Clone, Debug, Eq, Deserialize, PartialEq, Serialize)]
pub struct SdkModelEntry {
    /// SDK module name (e.g., "aws-sdk-s3" for S3)
    pub module: String,
    /// SDK module version number (e.g., "0.14.0")
    pub version: String,
    /// What changed
    pub kind: SdkModelChangeKind,
    /// More details about the change
    pub message: String,
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum ValidationSet {
    /// Validate for local development and CI
    Development,
    /// Validate for rendering.
    ///
    /// This does less validation to avoid blocking a release for things that
    /// were added to changelog validation later that could cause issues with
    /// SDK_CHANGELOG.next.json where there are historical entries that didn't
    /// have this validation applied.
    Render,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct Changelog {
    #[serde(rename = "smithy-rs")]
    #[serde(default)]
    pub smithy_rs: Vec<HandAuthoredEntry>,
    #[serde(rename = "aws-sdk-rust")]
    #[serde(default)]
    pub aws_sdk_rust: Vec<HandAuthoredEntry>,
    #[serde(rename = "aws-sdk-model")]
    #[serde(default)]
    pub sdk_models: Vec<SdkModelEntry>,
}

impl Changelog {
    pub fn new() -> Changelog {
        Default::default()
    }

    pub fn merge(&mut self, other: Changelog) {
        self.smithy_rs.extend(other.smithy_rs);
        self.aws_sdk_rust.extend(other.aws_sdk_rust);
        self.sdk_models.extend(other.sdk_models);
    }

    pub fn to_json_string(&self) -> Result<String> {
        serde_json::to_string_pretty(self).context("failed to serialize changelog JSON")
    }

    pub fn validate(&self, validation_set: ValidationSet) -> Result<(), Vec<String>> {
        let validate_aws_handauthored = |entry: &HandAuthoredEntry| -> Result<()> {
            entry.validate(validation_set)?;
            if entry.meta.target.is_some() {
                bail!("aws-sdk-rust changelog entry cannot have an affected target");
            }
            Ok(())
        };

        let validate_smithyrs_handauthored = |entry: &HandAuthoredEntry| -> Result<()> {
            entry.validate(validation_set)?;
            if entry.meta.target.is_none() {
                bail!("smithy-rs entry must have an affected target");
            }
            Ok(())
        };

        let errors: Vec<_> = self
            .aws_sdk_rust
            .iter()
            .map(validate_aws_handauthored)
            .chain(self.smithy_rs.iter().map(validate_smithyrs_handauthored))
            .filter_map(Result::err)
            .map(|e| format!("{e}"))
            .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[derive(Copy, Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum Target {
    #[serde(rename = "client")]
    Client,
    #[serde(rename = "server")]
    Server,
    #[serde(rename = "aws-sdk-rust")]
    AwsSdk,
}

impl FromStr for Target {
    type Err = anyhow::Error;

    fn from_str(sdk: &str) -> std::result::Result<Self, Self::Err> {
        match sdk.to_lowercase().as_str() {
            "client" => Ok(Target::Client),
            "server" => Ok(Target::Server),
            "aws-sdk-rust" => Ok(Target::AwsSdk),
            _ => bail!("An invalid type of `Target` {sdk} has been specified"),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct FrontMatter {
    pub applies_to: HashSet<Target>,
    pub authors: Vec<String>,
    pub references: Vec<Reference>,
    pub breaking: bool,
    pub new_feature: bool,
    pub bug_fix: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Markdown {
    pub front_matter: FrontMatter,
    pub message: String,
}

impl From<Markdown> for Changelog {
    fn from(value: Markdown) -> Self {
        let front_matter = value.front_matter;
        let entry = HandAuthoredEntry {
            message: value.message.trim_start().to_owned(),
            meta: Meta {
                bug: front_matter.bug_fix,
                breaking: front_matter.breaking,
                tada: front_matter.new_feature,
                target: None,
            },
            authors: AuthorsInner::Multiple(front_matter.authors).into(),
            references: front_matter.references,
            since_commit: None,
            age: None,
        };

        let mut changelog = Changelog::new();

        // Bin `entry` into the appropriate `Vec` based on the `applies_to` field in `front_matter`
        if front_matter.applies_to.contains(&Target::AwsSdk) {
            changelog.aws_sdk_rust.push(entry.clone())
        }
        if front_matter.applies_to.contains(&Target::Client)
            && front_matter.applies_to.contains(&Target::Server)
        {
            let mut entry = entry.clone();
            entry.meta.target = Some(SdkAffected::All);
            changelog.smithy_rs.push(entry);
        } else if front_matter.applies_to.contains(&Target::Client) {
            let mut entry = entry.clone();
            entry.meta.target = Some(SdkAffected::Client);
            changelog.smithy_rs.push(entry);
        } else if front_matter.applies_to.contains(&Target::Server) {
            let mut entry = entry.clone();
            entry.meta.target = Some(SdkAffected::Server);
            changelog.smithy_rs.push(entry);
        }

        changelog
    }
}
/// Parses changelog entries into [`Changelog`] using a series of parsers.
///
/// Each parser will attempt to parse an input string in order:
/// * If a parser successfully parses the input string into `Changelog`, it will be returned immediately.
///   No other parsers will be used.
/// * Otherwise, if a parser returns an `anyhow::Error`, the next parser will be tried.
/// * If none of the parsers parse the input string successfully, an error will be returned from the chain.
#[derive(Debug, Default)]
pub struct ChangelogLoader {
    parser_chain: ParserChain,
}

impl ChangelogLoader {
    /// Parses the given `value` into a `Changelog`
    pub fn parse_str(&self, value: impl AsRef<str>) -> Result<Changelog> {
        self.parser_chain.parse(value.as_ref())
    }

    /// Parses the contents of a file located at `path` into `Changelog`
    pub fn load_from_file(&self, path: impl AsRef<Path> + Debug) -> Result<Changelog> {
        let contents = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("failed to read {:?}", path.as_ref()))?;
        self.parse_str(contents)
            .with_context(|| format!("failed to parse the contents in {path:?}"))
    }

    /// Parses the contents of files stored in a directory `dir_path` into `Changelog`
    ///
    /// It opens each file in the directory, parses the file contents into `Changelog`,
    /// and merges it with accumulated `Changelog`. It currently does not support loading
    /// from recursive directory structures.
    pub fn load_from_dir(&self, dir_path: impl AsRef<Path> + Debug) -> Result<Changelog> {
        let entries = std::fs::read_dir(dir_path.as_ref())?;
        let result = entries
            .into_iter()
            .filter_map(|entry| {
                // Convert each entry to its path if it's a file
                entry.ok().and_then(|entry| {
                    let path = entry.path();
                    if path.is_file() {
                        Some(path)
                    } else {
                        None
                    }
                })
            })
            .try_fold(Changelog::new(), |mut combined_changelog, path| {
                combined_changelog.merge(self.load_from_file(path)?);
                Ok::<_, anyhow::Error>(combined_changelog)
            })?;

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context;

    #[test]
    fn errors_are_combined() {
        const ENTRY: &str = r#"
---
applies_to: ["aws-sdk-rust"]
authors: [""]
references: ["smithy-rs#920"]
breaking: false
new_feature: false
bug_fix: false
---
Fix typos in module documentation for generated crates
        "#;
        let loader = ChangelogLoader::default();
        let mut changelog = loader.parse_str(ENTRY).unwrap();
        changelog.merge(loader.parse_str(ENTRY).unwrap());
        // two errors should be produced, missing authors x 2
        let res = changelog.validate(ValidationSet::Development);
        assert_eq!(
            2,
            res.expect_err("changelog validation should fail").len()
        );
    }

    #[test]
    fn test_hand_authored_sdk() {
        let loader = ChangelogLoader::default();

        // server target
        let value = r#"
---
applies_to: ["server"]
authors: ["rcoh"]
references: ["smithy-rs#920"]
breaking: false
new_feature: false
bug_fix: false
---
Fix typos in module documentation for generated crates
"#;
        {
            let changelog = loader
                .parse_str(value)
                .context("String should have parsed")
                .unwrap();
            assert_eq!(
                Some(SdkAffected::Server),
                changelog.smithy_rs.first().unwrap().meta.target
            );
        }
        // client target
        let value = r#"
---
applies_to: ["client"]
authors: ["rcoh"]
references: ["smithy-rs#920"]
breaking: false
new_feature: false
bug_fix: false
---
Fix typos in module documentation for generated crates
"#;
        {
            let changelog = loader
                .parse_str(value)
                .context("String should have parsed")
                .unwrap();
            assert_eq!(
                Some(SdkAffected::Client),
                changelog.smithy_rs.first().unwrap().meta.target
            );
        }
        // Both target
        let value = r#"
---
applies_to: ["server", "client"]
authors: ["rcoh"]
references: ["smithy-rs#920"]
breaking: false
new_feature: false
bug_fix: false
---
Fix typos in module documentation for generated crates
"#;
        {
            let changelog = loader
                .parse_str(value)
                .context("String should have parsed")
                .unwrap();
            assert_eq!(
                Some(SdkAffected::All),
                changelog.smithy_rs.first().unwrap().meta.target
            );
        }
        // an invalid `applies_to` value
        let value = r#"
---
applies_to: ["Some other invalid"]
authors: ["rcoh"]
references: ["smithy-rs#920"]
breaking: false
new_feature: false
bug_fix: false
---
Fix typos in module documentation for generated crates
"#;
        {
            let changelog = loader
                .parse_str(value)
                .context("String should not have parsed");
            assert!(changelog.is_err());
        }
        // multiple authors
        let value = r#"
---
applies_to: ["client", "server"]
authors: ["rcoh", "crisidev"]
references: ["smithy-rs#920"]
breaking: false
new_feature: false
bug_fix: false
---
Fix typos in module documentation for generated crates
"#;
        {
            let changelog = loader
                .parse_str(value)
                .context("String should have parsed with multiple authors")
                .unwrap();
            assert_eq!(
                Authors(vec!["rcoh".to_string(), "crisidev".to_string()]),
                changelog.smithy_rs.first().unwrap().authors,
            );
        }
    }
}
