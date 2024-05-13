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
        let previous_release_commit = commit_for_tag(repo, &ancestor_tag)?;
        let previous_release_override_commit = commit_for_tag(repo, &tag_override)?;
        // The first guard says that whenever we override a previous release tag, `HEAD` of our branch
        // should be ahead of that override.
        // The second guard handles a case where our branch is now behind w.r.t the latest smithy-rs
        // release. This can happen when we `git merge main` into the branch, but the main branch
        // now contains a new smithy-rs release that the `HEAD` of the branch doesn't know about.
        // In that case, we want to teach the tool that the previous release should now be the new
        // release. However, specifying the new release for `previous_release_override_commit` fails
        // with the first guard because `HEAD` doesn't know about the new release. The second guard
        // provides an escape hatch where if `previous_release_commit` (the latest release currently
        // `HEAD` does know about) is the ancestor to the specified previous release override, then
        // we can now treat the previous release override as a legitimate previous release.
        if !is_ancestor(repo, &previous_release_override_commit, "HEAD")?
            && !is_ancestor(
                repo,
                &previous_release_commit,
                &previous_release_override_commit,
            )?
        {
            bail!("specified tag '{tag_override}' is neither the ancestor of HEAD nor a descendant of {ancestor_tag}");
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
    let tag = tag.trim();
    let maybe_release_tag = ReleaseTag::from_str(tag);
    let release_tag = match maybe_release_tag {
        Ok(tag) => Some(tag),
        Err(_) => strip_describe_tags_suffix(tag)
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
    tag.rsplitn(3, '-').nth(2)
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

/// True if the given `ancestor_commit` is an ancestor to `descendant_commit`
fn is_ancestor(repo: &Repo, ancestor_commit: &str, descendant_commit: &str) -> Result<bool> {
    let status = repo
        .git([
            "merge-base",
            "--is-ancestor",
            ancestor_commit,
            descendant_commit,
        ])
        .expect_status_one_of(
            "determine if {ancestor_commit} is the ancestor to {descendant_commit}",
            [0, 1],
        )?;
    Ok(status == 0)
}

/// Returns the commit hash for the given tag
fn commit_for_tag(repo: &Repo, tag: &ReleaseTag) -> Result<String> {
    repo.git(["rev-list", "-n1", tag.as_str()])
        .expect_success_output("retrieve commit hash for tag")
        .map(|result| result.trim().to_string())
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
