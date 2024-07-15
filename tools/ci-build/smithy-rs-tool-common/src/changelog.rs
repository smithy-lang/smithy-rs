/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! This module holds deserializable structs for the hand-authored changelog TOML files used in smithy-rs.

pub mod parser;

use crate::changelog::parser::{ParseIntoChangelog, ParserChain};
use anyhow::{bail, Context, Result};
use serde::{de, Deserialize, Deserializer, Serialize};
use std::fmt;
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

#[derive(Clone, Debug)]
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
                "Reference must of the form `repo#number` but found {}",
                reference
            ),
            Some((repo, number)) => {
                let number = number.parse::<usize>()?;
                if !matches!(repo, "smithy-rs" | "aws-sdk-rust" | "aws-sdk") {
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

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum AuthorsInner {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "AuthorsInner", into = "AuthorsInner")]
pub struct Authors(pub(super) Vec<String>);

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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum SdkModelChangeKind {
    Documentation,
    Feature,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
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
            .map(|e| format!("{}", e))
            .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
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
    pub fn load_from_file(&self, path: impl AsRef<Path>) -> Result<Changelog> {
        let contents = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("failed to read {:?}", path.as_ref()))?;
        self.parse_str(contents)
    }

    /// Parses the contents of files stored in a directory `dir_path` into `Changelog`
    ///
    /// It opens each file in the directory, parses the file contents into `Changelog`,
    /// and merges it with accumulated `Changelog`. It currently does not support loading
    /// from recursive directory structures.
    pub fn load_from_dir(&self, dir_path: impl AsRef<Path>) -> Result<Changelog> {
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
    use crate::changelog::parser::Toml;
    use anyhow::Context;

    #[test]
    fn errors_are_combined() {
        let buffer = r#"
            [[aws-sdk-rust]]
            message = "Fix typos in module documentation for generated crates"
            references = ["smithy-rs#920"]
            meta = { "breaking" = false, "tada" = false, "bug" = false }
            author = ""
            [[smithy-rs]]
            message = "Fix typos in module documentation for generated crates"
            references = ["smithy-rs#920"]
            meta = { "breaking" = false, "tada" = false, "bug" = false }
            author = ""
            [[smithy-rs]]
            message = "Fix typos in module documentation for generated crates"
            references = ["smithy-rs#920"]
            meta = { "breaking" = false, "tada" = false, "bug" = false }
            author = "fz"
        "#;
        // three errors should be produced, missing authors x 2 and a SdkAffected is not set to default
        let changelog: Changelog = toml::from_str(buffer).expect("valid changelog");
        let res = changelog.validate(ValidationSet::Development);
        assert!(res.is_err());
        if let Err(e) = res {
            assert_eq!(3, e.len());
            assert!(e.contains(&"smithy-rs entry must have an affected target".to_string()))
        }
    }

    // TODO(file-per-change-changelog): Remove this test once we have switched to the new markdown
    //  format because targets will be explicit and there won't be defaults set.
    #[test]
    fn confirm_smithy_rs_defaults_set() {
        let buffer = r#"
            [[aws-sdk-rust]]
            message = "Fix typos in module documentation for generated crates"
            references = ["smithy-rs#920"]
            meta = { "breaking" = false, "tada" = false, "bug" = false }
            author = "rcoh"
            [[smithy-rs]]
            message = "Fix typos in module documentation for generated crates"
            references = ["smithy-rs#920"]
            meta = { "breaking" = false, "tada" = false, "bug" = false }
            author = "rcoh"
            [[smithy-rs]]
            message = "Fix typos in module documentation for generated crates"
            references = ["smithy-rs#920"]
            meta = { "breaking" = false, "tada" = false, "bug" = false }
            author = "fz"
        "#;
        {
            // parsing directly using `toml::from_str` won't set the default target field
            let changelog: Changelog = toml::from_str(buffer).expect("valid changelog");
            let res = changelog.validate(ValidationSet::Development);
            assert!(res.is_err());
            if let Err(e) = res {
                assert!(e.contains(&"smithy-rs entry must have an affected target".to_string()))
            }
        }
        {
            // parsing through the `Toml` parser will result in no error
            let changelog: Changelog = Toml::default().parse(buffer).expect("valid changelog");
            let res = changelog.validate(ValidationSet::Development);
            assert!(res.is_ok());
            if let Err(e) = res {
                panic!("some error has been produced {e:?}");
            }
            assert_eq!(changelog.smithy_rs[1].meta.target, Some(SdkAffected::All));
        }
    }

    #[test]
    fn test_hand_authored_sdk() {
        // server target
        let value = r#"
            message = "Fix typos in module documentation for generated crates"
            references = ["smithy-rs#920"]
            meta = { "breaking" = false, "tada" = false, "bug" = false, "target" = "Server" }
            author = "rcoh"
        "#;
        {
            let value: HandAuthoredEntry = toml::from_str(value)
                .context("String should have parsed")
                .unwrap();
            assert_eq!(Some(SdkAffected::Server), value.meta.target);
        }

        // client target
        let value = r#"
            message = "Fix typos in module documentation for generated crates"
            references = ["smithy-rs#920"]
            meta = { "breaking" = false, "tada" = false, "bug" = false, "target" = "Client" }
            author = "rcoh"
        "#;
        {
            let value: HandAuthoredEntry = toml::from_str(value)
                .context("String should have parsed")
                .unwrap();
            assert_eq!(Some(SdkAffected::Client), value.meta.target);
        }
        // Both target
        let value = r#"
            message = "Fix typos in module documentation for generated crates"
            references = ["smithy-rs#920"]
            meta = { "breaking" = false, "tada" = false, "bug" = false, "target" = "all" }
            author = "rcoh"
        "#;
        {
            let value: HandAuthoredEntry = toml::from_str(value)
                .context("String should have parsed")
                .unwrap();
            assert_eq!(Some(SdkAffected::All), value.meta.target);
        }
        // an invalid sdk value
        let value = r#"
            message = "Fix typos in module documentation for generated crates"
            references = ["smithy-rs#920"]
            meta = { "breaking" = false, "tada" = false, "bug" = false, "target" = "Some other invalid" }
            author = "rcoh"
        "#;
        {
            let value: Result<HandAuthoredEntry, _> =
                toml::from_str(value).context("String should not have parsed");
            assert!(value.is_err());
        }
        // missing sdk in the meta tag
        let value = r#"
            message = "Fix typos in module documentation for generated crates"
            references = ["smithy-rs#920"]
            meta = { "breaking" = false, "tada" = false, "bug" = false }
            author = "rcoh"
        "#;
        {
            let value: HandAuthoredEntry = toml::from_str(value)
                .context("String should have parsed as it has none meta.sdk")
                .unwrap();
            assert_eq!(None, value.meta.target);
        }
        // single author
        let value = r#"
            message = "Fix typos in module documentation for generated crates"
            references = ["smithy-rs#920"]
            meta = { "breaking" = false, "tada" = false, "bug" = false }
            author = "rcoh"
        "#;
        {
            let value: HandAuthoredEntry = toml::from_str(value)
                .context("String should have parsed with multiple authors")
                .unwrap();
            assert_eq!(Authors(vec!["rcoh".to_string()]), value.authors);
        }
        // multiple authors
        let value = r#"
            message = "Fix typos in module documentation for generated crates"
            references = ["smithy-rs#920"]
            meta = { "breaking" = false, "tada" = false, "bug" = false }
            authors = ["rcoh", "crisidev"]
        "#;
        {
            let value: HandAuthoredEntry = toml::from_str(value)
                .context("String should have parsed with multiple authors")
                .unwrap();
            assert_eq!(
                Authors(vec!["rcoh".to_string(), "crisidev".to_string()]),
                value.authors
            );
        }
    }
}
