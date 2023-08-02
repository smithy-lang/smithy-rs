/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! This module holds deserializable structs for the hand-authored changelog TOML files used in smithy-rs.

use anyhow::{bail, Context, Result};
use serde::{de, Deserialize, Deserializer, Serialize};
use std::fmt::{self, Display};
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

impl Display for SdkAffected {
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

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct HandAuthoredEntry {
    pub message: String,
    pub meta: Meta,
    pub author: String,
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
    pub fn validate(&self) -> Result<()> {
        if self.author.is_empty() {
            bail!("Author must be set (was empty)");
        }
        if !self.author.chars().all(|c| c.is_alphanumeric() || c == '-') {
            bail!("Author must be valid GitHub username: [a-zA-Z0-9\\-]")
        }
        // TODO(enableNewSmithyRuntimeCleanup): Re-add this validation
        // if self.references.is_empty() {
        //     bail!("Changelog entry must refer to at least one pull request or issue");
        // }

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

    pub fn parse_str(value: &str) -> Result<Changelog> {
        let mut changelog: Changelog =
            (match toml::from_str(value).context("Invalid TOML changelog format") {
                Ok(parsed) => Ok(parsed),
                Err(toml_err) => {
                    // Remove comments from the top
                    let value = value
                        .split('\n')
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
            } as Result<Changelog>)?;
        // all smithry-rs entries should have meta.target set to the default value instead of None
        for entry in &mut changelog.smithy_rs {
            if entry.meta.target.is_none() {
                entry.meta.target = Some(SdkAffected::default());
            }
        }
        Ok(changelog)
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
        let validate_aws_handauthored = |entry: &HandAuthoredEntry| -> Result<()> {
            entry.validate()?;
            if entry.meta.target.is_some() {
                bail!("aws-sdk-rust changelog entry cannot have an affected target");
            }
            Ok(())
        };

        let validate_smithyrs_handauthored = |entry: &HandAuthoredEntry| -> Result<()> {
            entry.validate()?;
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

#[cfg(test)]
mod tests {
    use super::{Changelog, HandAuthoredEntry, SdkAffected};
    use anyhow::Context;

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
        let res = changelog.validate();
        assert!(res.is_err());
        if let Err(e) = res {
            assert_eq!(e.len(), 3);
            assert!(e.contains(&"smithy-rs entry must have an affected target".to_string()))
        }
    }

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
            // loading directly from toml::from_str won't set the default target field
            let changelog: Changelog = toml::from_str(buffer).expect("valid changelog");
            let res = changelog.validate();
            assert!(res.is_err());
            if let Err(e) = res {
                assert!(e.contains(&"smithy-rs entry must have an affected target".to_string()))
            }
        }
        {
            // loading through Chanelog will result in no error
            let changelog: Changelog = Changelog::parse_str(buffer).expect("valid changelog");
            let res = changelog.validate();
            assert!(res.is_ok());
            if let Err(e) = res {
                panic!("some error has been produced {e:?}");
            }
            assert_eq!(changelog.smithy_rs[1].meta.target, Some(SdkAffected::All));
        }
    }

    #[test]
    fn parse_smithy_ok() {
        // by default smithy-rs meta data should say change is for both
        let toml = r#"
            [[aws-sdk-rust]]
            message = "Fix typos in module documentation for generated crates"
            references = ["smithy-rs#920"]
            meta = { "breaking" = false, "tada" = false, "bug" = false }
            author = "rcoh"
            [[smithy-rs]]
            message = "Fix typos in module documentation for generated crates"
            references = ["smithy-rs#920"]
            meta = { "breaking" = false, "tada" = false, "bug" = false, "target" = "client" }
            author = "rcoh"
            [[smithy-rs]]
            message = "Fix typos in module documentation for generated crates"
            references = ["smithy-rs#920"]
            meta = { "breaking" = false, "tada" = false, "bug" = false, "target" = "server" }
            author = "rcoh"
            [[smithy-rs]]
            message = "Fix typos in module documentation for generated crates"
            references = ["smithy-rs#920"]
            meta = { "breaking" = false, "tada" = false, "bug" = false, "target" = "all" }
            author = "rcoh"
            [[smithy-rs]]
            message = "Fix typos in module documentation for generated crates"
            references = ["smithy-rs#920"]
            meta = { "breaking" = false, "tada" = false, "bug" = false }
            author = "rcoh"
        "#;
        let changelog = Changelog::parse_str(toml).unwrap();
        assert_eq!(changelog.smithy_rs.len(), 4);
        assert_eq!(
            changelog.smithy_rs[0].meta.target,
            Some(SdkAffected::Client)
        );
        assert_eq!(
            changelog.smithy_rs[1].meta.target,
            Some(SdkAffected::Server)
        );
        assert_eq!(changelog.smithy_rs[2].meta.target, Some(SdkAffected::All));
        assert_eq!(changelog.smithy_rs[3].meta.target, Some(SdkAffected::All));
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
            assert_eq!(value.meta.target, Some(SdkAffected::Server));
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
            assert_eq!(value.meta.target, Some(SdkAffected::Client));
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
            assert_eq!(value.meta.target, Some(SdkAffected::All));
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
            assert_eq!(value.meta.target, None);
        }
    }
}
