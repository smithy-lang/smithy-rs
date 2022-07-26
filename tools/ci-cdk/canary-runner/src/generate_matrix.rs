/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use serde::Serialize;
use std::cmp::Ordering;
use std::time::Duration;

const KNOWN_YANKED_RELEASE_TAGS: &[&str] = &[
    // leave this one in here for unit tests
    "test-yanked-release",
    // add release tags here to get the canary passing after yanking a release
];

#[derive(Debug, Parser)]
pub struct GenerateMatrixOpt {
    /// Number of previous SDK versions to run the canary against
    #[clap(short, long)]
    sdk_versions: u8,

    /// Versions of Rust to compile against
    #[clap(short, long, multiple_values = true)]
    rust_versions: Vec<String>,
}

#[derive(Debug, Serialize)]
struct Output {
    sdk_release_tags: Vec<String>,
    rust_version: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Release {
    timestamp: Option<i64>,
    tag_name: String,
}

impl Ord for Release {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}

impl PartialOrd for Release {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // Reverse chronological order
        match other.timestamp.partial_cmp(&self.timestamp) {
            Some(core::cmp::Ordering::Equal) => self.tag_name.partial_cmp(&other.tag_name),
            ord => ord,
        }
    }
}

// Use a trait to make unit testing the GitHub API pagination easier
#[async_trait]
trait RetrieveReleases {
    async fn retrieve(&self, owner: &str, repo: &str, page_num: i64) -> Result<Vec<Release>>;
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
    async fn retrieve(&self, owner: &str, repo: &str, page_num: i64) -> Result<Vec<Release>> {
        let result = self
            .github
            .repos()
            .list_releases(owner, repo, 100, page_num)
            .await?
            .into_iter()
            .map(|release| Release {
                timestamp: release.published_at.map(|d| d.timestamp()),
                tag_name: release.tag_name,
            })
            .collect();
        // Be nice to GitHub
        tokio::time::sleep(Duration::from_millis(250)).await;
        Ok(result)
    }
}

async fn retrieve_latest_release_tags(
    retrieve_releases: &dyn RetrieveReleases,
    desired: usize,
) -> Result<Vec<String>> {
    // The GitHub API doesn't document the order that releases are returned in,
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

    Ok(releases
        .into_iter()
        .filter(
            |Release {
                 timestamp,
                 tag_name,
             }| {
                timestamp.is_some() && !KNOWN_YANKED_RELEASE_TAGS.contains(&tag_name.as_str())
            },
        )
        .take(desired)
        .map(|release| release.tag_name)
        .collect())
}

pub async fn generate_matrix(opt: GenerateMatrixOpt) -> Result<()> {
    let retrieve_releases = GitHubRetrieveReleases::new()?;
    let sdk_release_tags =
        retrieve_latest_release_tags(&retrieve_releases, opt.sdk_versions as usize).await?;

    let output = Output {
        sdk_release_tags,
        rust_version: opt.rust_versions,
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestRetriever {
        pages: Vec<Vec<Release>>,
    }

    #[async_trait]
    impl RetrieveReleases for TestRetriever {
        async fn retrieve(&self, owner: &str, repo: &str, page_num: i64) -> Result<Vec<Release>> {
            assert_eq!("awslabs", owner);
            assert_eq!("aws-sdk-rust", repo);
            Ok(self.pages[(page_num - 1) as usize].clone())
        }
    }

    #[tokio::test]
    async fn test_retrieve_latest_release_tags() {
        let retriever = TestRetriever {
            pages: vec![
                vec![
                    Release {
                        timestamp: Some(200),
                        tag_name: "release-200".into(),
                    },
                    Release {
                        timestamp: None,
                        tag_name: "draft-release-1".into(),
                    },
                    Release {
                        timestamp: Some(300),
                        tag_name: "release-300".into(),
                    },
                    Release {
                        timestamp: None,
                        tag_name: "draft-release-2".into(),
                    },
                ],
                vec![
                    Release {
                        timestamp: None,
                        tag_name: "draft-release-3".into(),
                    },
                    Release {
                        timestamp: None,
                        tag_name: "draft-release-4".into(),
                    },
                    Release {
                        timestamp: Some(500),
                        tag_name: "release-500".into(),
                    },
                    Release {
                        timestamp: Some(600),
                        tag_name: "test-yanked-release".into(),
                    },
                ],
                vec![
                    Release {
                        timestamp: None,
                        tag_name: "draft-release-5".into(),
                    },
                    Release {
                        timestamp: Some(100),
                        tag_name: "release-100".into(),
                    },
                    Release {
                        timestamp: Some(400),
                        tag_name: "release-400".into(),
                    },
                ],
                vec![],
            ],
        };

        let tags = retrieve_latest_release_tags(&retriever, 3).await.unwrap();
        assert_eq!(
            vec![
                "release-500".to_string(),
                "release-400".into(),
                "release-300".into()
            ],
            tags
        );
    }
}
