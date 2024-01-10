/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::repo::Repo;
use anyhow::{anyhow, bail, Context, Result};
use smithy_rs_tool_common::{command::sync::CommandExt, release_tag::ReleaseTag};
use std::str::FromStr;
use tracing::warn;

/// Discovers and returns the tag of the previous release for the revision at HEAD.
pub fn previous_release_tag(
    repo: &Repo,
    release_tags: &[ReleaseTag],
    release_tag_override: Option<&str>,
) -> Result<ReleaseTag> {
    let ancestor_tag = ancestor_tag(repo)?;
    let tag_override = release_tag_override
        .map(ReleaseTag::from_str)
        .transpose()
        .context("invalid release tag given")?;
    if let Some(tag_override) = tag_override {
        if !release_tags.contains(&tag_override) {
            bail!("specified tag '{tag_override}' doesn't exist");
        }
        if !tag_is_ancestor(repo, &tag_override)? {
            bail!("specified tag '{tag_override}' is not an ancestor to HEAD");
        }
        if tag_override != ancestor_tag {
            warn!(
                "expected previous release to be '{ancestor_tag}', \
                 but '{tag_override}' was specified. Proceeding with '{tag_override}'.",
            );
        }
        Ok(tag_override)
    } else {
        Ok(ancestor_tag)
    }
}

fn ancestor_tag(repo: &Repo) -> Result<ReleaseTag> {
    let tag = repo
        .git(["describe", "--tags"])
        .expect_success_output("find the current ancestor release tag")?;
    let maybe_release_tag = ReleaseTag::from_str(&tag);
    let release_tag = match maybe_release_tag {
        Ok(tag) => Some(tag),
        Err(_) => strip_describe_tags_suffix(&tag)
            .map(ReleaseTag::from_str)
            .transpose()
            .context("failed to find ancestor release tag")?,
    };
    release_tag.ok_or_else(|| anyhow!("failed to find ancestor release tag"))
}

// `git describe --tags` appends a suffix if the current commit is not the tagged commit
//
// Function assumes the given tag is known to be suffixed.
fn strip_describe_tags_suffix(tag: &str) -> Option<&str> {
    // Example release tag with suffix: release-2023-12-01-42-g885048e40
    let end_index = tag
        .char_indices()
        .rev()
        .skip_while(|(_, c)| *c != '-')
        .skip(1)
        .find(|(_, c)| *c == '-')
        .map(|(i, _)| i);
    if let Some(end_index) = end_index {
        Some(&tag[0..end_index])
    } else {
        None
    }
}

/// Returns all release tags for the repo in descending order by time.
pub fn release_tags(repo: &Repo) -> Result<Vec<ReleaseTag>> {
    let mut tags: Vec<_> = repo
        .git(["tag"])
        .expect_success_output("find the current ancestor release tag")?
        .lines()
        .flat_map(|tag| match ReleaseTag::from_str(tag) {
            Ok(tag) => Some(tag),
            Err(_) => {
                if !tag.starts_with("v0.") {
                    warn!("ignoring tag '{tag}': doesn't look like a release tag");
                }
                None
            }
        })
        .collect();
    tags.sort_by(|a, b| b.cmp(a));
    Ok(tags)
}

/// True if the given tag is an ancestor to HEAD.
fn tag_is_ancestor(repo: &Repo, tag: &ReleaseTag) -> Result<bool> {
    let commit = commit_for_tag(repo, tag)?;
    let status = repo
        .git(["merge-base", "--is-ancestor", &commit, "HEAD"])
        .expect_status_one_of("determine if a tag is the ancestor to HEAD", [0, 1])?;
    Ok(status == 0)
}

/// Returns the commit hash for the given tag
fn commit_for_tag(repo: &Repo, tag: &ReleaseTag) -> Result<String> {
    repo.git(["rev-list", "-n1", tag.as_str()])
        .expect_success_output("retrieve commit hash for tag")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_git_describe_tags_suffix() {
        assert_eq!(
            Some("release-2023-12-01"),
            strip_describe_tags_suffix("release-2023-12-01-42-g885048e40")
        );
        assert_eq!(
            Some("release-2023-12-01"),
            strip_describe_tags_suffix("release-2023-12-01-2-g885048e40")
        );
        assert_eq!(
            Some("release-2023-12-01"),
            strip_describe_tags_suffix("release-2023-12-01-123-g885048e40")
        );
        assert_eq!(None, strip_describe_tags_suffix("invalid"));
    }
}
