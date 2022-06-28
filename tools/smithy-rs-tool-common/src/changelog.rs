/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! This module holds deserializable structs for the hand-authored changelog TOML files used in smithy-rs.

use anyhow::{bail, Context, Result};
use serde::{de, Deserialize, Deserializer, Serialize};
use std::fmt;
use std::path::Path;
use std::str::FromStr;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Meta {
    pub bug: bool,
    pub breaking: bool,
    pub tada: bool,
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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HandAuthoredEntry {
    pub message: String,
    pub meta: Meta,
    pub author: String,
    #[serde(default)]
    pub references: Vec<Reference>,
    /// Optional commit hash to indicate "since when" these changes were made
    #[serde(rename = "since-commit")]
    pub since_commit: Option<String>,
}

impl HandAuthoredEntry {
    /// Validate a changelog entry to ensure it follows standards
    pub fn validate(&self) -> Result<()> {
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
        self.smithy_rs.extend(other.smithy_rs.into_iter());
        self.aws_sdk_rust.extend(other.aws_sdk_rust.into_iter());
        self.sdk_models.extend(other.sdk_models.into_iter());
    }

    fn parse_str(value: &str) -> Result<Changelog> {
        match toml::from_str(value).context("Invalid TOML changelog format") {
            Ok(parsed) => Ok(parsed),
            Err(toml_err) => {
                // Remove comments from the top
                let value = value
                    .split('\n')
                    .into_iter()
                    .filter(|line| !line.trim().starts_with('#'))
                    .collect::<Vec<_>>()
                    .join("\n");
                match serde_json::from_str(&value).context("Invalid JSON changelog format") {
                    Ok(parsed) => Ok(parsed),
                    Err(json_err) => bail!(
                        "Invalid JSON or TOML changelog format:\n{:?}\n{:?}",
                        toml_err,
                        json_err
                    ),
                }
            }
        }
    }

    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Changelog> {
        let contents = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("failed to read {:?}", path.as_ref()))?;
        Self::parse_str(&contents)
    }

    pub fn to_json_string(&self) -> Result<String> {
        serde_json::to_string_pretty(self).context("failed to serialize changelog JSON")
    }

    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = vec![];
        for entry in self.aws_sdk_rust.iter().chain(self.smithy_rs.iter()) {
            if let Err(e) = entry.validate() {
                errors.push(format!("{}", e));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Changelog;

    #[test]
    fn parse_json() {
        let json = r#"
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
            {
                "smithy-rs": [],
                "aws-sdk-rust": [
                    {
                        "message": "Some change",
                        "meta": { "bug": true, "breaking": false, "tada": false },
                        "author": "test-dev",
                        "references": [
                            "aws-sdk-rust#123",
                            "smithy-rs#456"
                        ]
                    }
                ],
                "aws-sdk-model": [
                    {
                        "module": "aws-sdk-ec2",
                        "version": "0.12.0",
                        "kind": "Feature",
                        "message": "Some API change"
                    }
                ]
            }
        "#;
        let changelog = Changelog::parse_str(json).unwrap();
        assert!(changelog.smithy_rs.is_empty());
        assert_eq!(1, changelog.aws_sdk_rust.len());
        assert_eq!("Some change", changelog.aws_sdk_rust[0].message);
        assert_eq!(1, changelog.sdk_models.len());
        assert_eq!("Some API change", changelog.sdk_models[0].message);
    }
}
