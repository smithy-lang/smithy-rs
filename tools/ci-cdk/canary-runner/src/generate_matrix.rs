/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use serde::Serialize;
use smithy_rs_tool_common::release_tag::ReleaseTag;
use std::str::FromStr;
use std::time::Duration;

const KNOWN_YANKED_RELEASE_TAGS: &[&str] = &[
    // Test release tag for the unit tests.
    // There wasn't a release on this date, so this is fine for testing
    "release-2022-07-04",
    // Add release tags here to get the canary passing after yanking a release
    "release-2022-10-13",
    "release-2022-12-15", // Failed release
    "release-2022-12-16", // Failed release
];

#[derive(Debug, Parser, Eq, PartialEq)]
pub struct GenerateMatrixArgs {
    /// Number of previous SDK versions to run the canary against
    #[clap(short, long)]
    sdk_versions: u8,

    /// Versions of Rust to compile against
    #[clap(short, long, multiple_values = true)]
    rust_versions: Vec<String>,

    /// Target architectures to test
    #[clap(short, long, multiple_values = true)]
    architectures: Vec<String>,
}

#[derive(Debug, Serialize)]
struct Output {
    sdk_release_tag: Vec<String>,
    rust_version: Vec<String>,
    target_arch: Vec<String>,
}

// Use a trait to make unit testing the GitHub API pagination easier
#[async_trait]
trait RetrieveReleases {
    async fn retrieve(&self, owner: &str, repo: &str, page_num: i64) -> Result<Vec<ReleaseTag>>;
}

struct GitHubRetrieveReleases {
    github: octorust::Client,
}

impl GitHubRetrieveReleases {
    fn new() -> Result<Self> {
        let user_agent = "smithy-rs-canary-runner-generate-matrix";
        Ok(Self {
            github: octorust::Client::new(user_agent, None)?,
        })
    }
}

#[async_trait]
impl RetrieveReleases for GitHubRetrieveReleases {
    async fn retrieve(&self, owner: &str, repo: &str, page_num: i64) -> Result<Vec<ReleaseTag>> {
        let result = self
            .github
            .repos()
            .list_tags(owner, repo, 100, page_num)
            .await?
            .body
            .into_iter()
            .filter_map(|tag| ReleaseTag::from_str(&tag.name).ok())
            .collect();
        // Be nice to GitHub
        tokio::time::sleep(Duration::from_millis(250)).await;
        Ok(result)
    }
}

async fn retrieve_latest_release_tags(
    retrieve_releases: &dyn RetrieveReleases,
    desired: usize,
) -> Result<Vec<ReleaseTag>> {
    // The GitHub API doesn't document the order that tags are returned in,
    // so assume random order and sort them ourselves.
    let mut page_num = 1;
    let mut releases = Vec::new();
    loop {
        let page = retrieve_releases
            .retrieve("awslabs", "aws-sdk-rust", page_num)
            .await?;
        if page.is_empty() {
            break;
        }
        releases.extend(page.into_iter());
        page_num += 1;
    }
    releases.sort();
    releases.reverse();

    Ok(releases
        .into_iter()
        .filter(|release_tag| !KNOWN_YANKED_RELEASE_TAGS.contains(&release_tag.as_str()))
        .take(desired)
        .collect())
}

pub async fn generate_matrix(opt: GenerateMatrixArgs) -> Result<()> {
    let retrieve_releases = GitHubRetrieveReleases::new()?;
    let sdk_release_tags =
        retrieve_latest_release_tags(&retrieve_releases, opt.sdk_versions as usize).await?;

    let output = Output {
        sdk_release_tag: sdk_release_tags
            .into_iter()
            .map(|t| t.to_string())
            .collect(),
        rust_version: opt.rust_versions,
        target_arch: opt.architectures,
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(value: &str) -> ReleaseTag {
        ReleaseTag::from_str(value).unwrap()
    }

    struct TestRetriever {
        pages: Vec<Vec<ReleaseTag>>,
    }

    #[async_trait]
    impl RetrieveReleases for TestRetriever {
        async fn retrieve(
            &self,
            owner: &str,
            repo: &str,
            page_num: i64,
        ) -> Result<Vec<ReleaseTag>> {
            assert_eq!("awslabs", owner);
            assert_eq!("aws-sdk-rust", repo);
            Ok(self.pages[(page_num - 1) as usize].clone())
        }
    }

    #[tokio::test]
    async fn test_retrieve_latest_release_tags() {
        let retriever = TestRetriever {
            pages: vec![
                vec![tag("v0.12.0"), tag("v0.15.0"), tag("release-2022-07-03")],
                vec![tag("v0.14.0"), tag("release-2022-07-05"), tag("v0.11.0")],
                vec![
                    tag("v0.13.0"),
                    tag("release-2022-07-01"),
                    tag("release-2022-07-04"),
                ],
                vec![],
            ],
        };

        let tags = retrieve_latest_release_tags(&retriever, 4).await.unwrap();
        assert_eq!(
            vec![
                tag("release-2022-07-05"),
                tag("release-2022-07-03"),
                tag("release-2022-07-01"),
                tag("v0.15.0"),
            ],
            tags
        );
    }
}
