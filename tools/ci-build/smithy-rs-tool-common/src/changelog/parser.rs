/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::changelog::{Changelog, Markdown};
use anyhow::{bail, Context, Result};
use std::fmt::Debug;

pub(crate) trait ParseIntoChangelog: Debug {
    fn parse(&self, value: &str) -> Result<Changelog>;
}

#[derive(Clone, Debug, Default)]
pub(super) struct Toml;
impl ParseIntoChangelog for Toml {
    fn parse(&self, value: &str) -> Result<Changelog> {
        toml::from_str(value).context("Invalid TOML changelog format")
    }
}

#[derive(Clone, Debug, Default)]
struct Json;
impl ParseIntoChangelog for Json {
    fn parse(&self, value: &str) -> Result<Changelog> {
        // Remove comments from the top
        let value = value
            .split('\n')
            .filter(|line| !line.trim().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        serde_json::from_str(&value).context("Invalid JSON changelog format")
    }
}

impl ParseIntoChangelog for Markdown {
    fn parse(&self, value: &str) -> Result<Changelog> {
        let mut parts = value.splitn(3, "---");
        let _ = parts.next(); // Skip first empty element
        let front_matter_str = parts
            .next()
            .context("front matter should follow the opening `---`")?;
        let message = parts
            .next()
            .context("message should be included in changelog entry")?;

        let markdown = Markdown {
            front_matter: serde_yaml::from_str(front_matter_str)?,
            message: message.to_owned(),
        };

        Ok(markdown.into())
    }
}

#[derive(Debug)]
pub(crate) struct ParserChain {
    parsers: Vec<(&'static str, Box<dyn ParseIntoChangelog>)>,
}

impl Default for ParserChain {
    fn default() -> Self {
        Self {
            parsers: vec![
                ("markdown", Box::<Markdown>::default()),
                ("toml", Box::<Toml>::default()),
                ("json", Box::<Json>::default()),
            ],
        }
    }
}

impl ParseIntoChangelog for ParserChain {
    fn parse(&self, value: &str) -> Result<Changelog> {
        let mut errs = Vec::new();
        for (_name, parser) in &self.parsers {
            match parser.parse(value) {
                Ok(parsed) => {
                    return Ok(parsed);
                }
                Err(err) => {
                    errs.push(err.to_string());
                }
            }
        }
        bail!("no parsers in chain parsed the following into `Changelog`\n{value}\nwith respective errors: \n{errs:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changelog::{SdkAffected, SdkModelChangeKind, SdkModelEntry};

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
        let changelog = Json.parse(json).unwrap();
        assert!(changelog.smithy_rs.is_empty());
        assert_eq!(1, changelog.aws_sdk_rust.len());
        assert_eq!("Some change", changelog.aws_sdk_rust[0].message);
        assert_eq!(1, changelog.sdk_models.len());
        assert_eq!("Some API change", changelog.sdk_models[0].message);
    }

    #[test]
    fn parse_toml() {
        let toml = r#"
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
        "#;
        let changelog = Toml.parse(toml).unwrap();
        assert_eq!(2, changelog.sdk_models.len());
        assert_eq!(
            SdkModelEntry {
                module: "aws-sdk-s3".to_string(),
                version: "0.14.0".to_string(),
                kind: SdkModelChangeKind::Feature,
                message: "Some new API to do X".to_string(),
            },
            changelog.sdk_models[0]
        );
        assert_eq!(
            SdkModelEntry {
                module: "aws-sdk-ec2".to_string(),
                version: "0.12.0".to_string(),
                kind: SdkModelChangeKind::Documentation,
                message: "Updated some docs".to_string(),
            },
            changelog.sdk_models[1]
        );
    }

    #[test]
    fn parse_markdown() {
        {
            let markdown = r#"---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["landonxjames","todaaron"]
references: ["smithy-rs#123"]
breaking: false
new_feature: false
bug_fix: false
---
# Markdown Content
This is some **Markdown** content.
"#;
            let changelog = Markdown::default().parse(markdown).unwrap();
            assert_eq!(1, changelog.smithy_rs.len());
            assert_eq!(Some(SdkAffected::All), changelog.smithy_rs[0].meta.target);
            assert_eq!(
                "# Markdown Content\nThis is some **Markdown** content.\n",
                &changelog.smithy_rs[0].message
            );
            // Should duplicate this entry into the SDK changelog by virtue of `aws-sdk-rust`
            assert_eq!(1, changelog.aws_sdk_rust.len());
        }
        {
            let markdown = r#"---
applies_to: ["client"]
authors: ["velfi"]
references: ["smithy-rs#456", "aws-sdk-rust#1234"]
breaking: false
new_feature: false
bug_fix: false
---
# Markdown Content
This is some **Markdown** content.
"#;
            let changelog = Markdown::default().parse(markdown).unwrap();
            assert_eq!(1, changelog.smithy_rs.len());
            assert_eq!(
                Some(SdkAffected::Client),
                changelog.smithy_rs[0].meta.target
            );
            assert_eq!(
                "# Markdown Content\nThis is some **Markdown** content.\n",
                &changelog.smithy_rs[0].message
            );
            assert!(changelog.aws_sdk_rust.is_empty());
        }
        {
            let markdown = r#"---
applies_to: ["server"]
authors: ["david-perez", "drganjoo"]
references: ["smithy-rs#789"]
breaking: false
new_feature: false
bug_fix: true
---
# Markdown Content
This is some **Markdown** content.
"#;
            let changelog = Markdown::default().parse(markdown).unwrap();
            assert_eq!(1, changelog.smithy_rs.len());
            assert_eq!(
                Some(SdkAffected::Server),
                changelog.smithy_rs[0].meta.target
            );
            assert_eq!(
                "# Markdown Content\nThis is some **Markdown** content.\n",
                &changelog.smithy_rs[0].message
            );
            assert!(changelog.aws_sdk_rust.is_empty());
        }
        {
            let markdown = r#"---
applies_to:
- client
- aws-sdk-rust
authors:
- ysaito1001
references:
- smithy-rs#1234
breaking: false
new_feature: false
bug_fix: true
---
Fix a long-standing bug.
"#;
            let changelog = Markdown::default().parse(markdown).unwrap();
            assert_eq!(1, changelog.smithy_rs.len());
            assert_eq!(
                Some(SdkAffected::Client),
                changelog.smithy_rs[0].meta.target
            );
            assert_eq!(
                "Fix a long-standing bug.\n",
                &changelog.smithy_rs[0].message
            );
            assert_eq!(1, changelog.aws_sdk_rust.len());
        }
    }
}
