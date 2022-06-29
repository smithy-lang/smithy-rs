/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::{bail, Result};
use clap::clap_derive::ArgEnum;
use smithy_rs_tool_common::changelog::{Changelog, HandAuthoredEntry, SdkModelEntry, SdkAffected};
use smithy_rs_tool_common::git::Git;
use smithy_rs_tool_common::versions_manifest::VersionsManifest;
use std::path::Path;

#[derive(ArgEnum, Copy, Clone, Debug, Eq, PartialEq)]
pub enum ChangeSet {
    SmithyRs,
    AwsSdk,
}

pub struct ChangelogEntries {
    pub aws_sdk_rust: Vec<ChangelogEntry>,
    pub smithy_rs: Vec<ChangelogEntry>,
}

impl ChangelogEntries {
    pub fn filter(
        self,
        smithy_rs: &dyn Git,
        smithy_rs_sdk : Option<SdkAffected>,
        change_set: ChangeSet,
        previous_release_versions_manifest: Option<&Path>,
    ) -> Result<Vec<ChangelogEntry>> {
        match change_set {
            ChangeSet::AwsSdk => {
                if let Some(manifest_path) = previous_release_versions_manifest {
                    let manifest = VersionsManifest::from_file(manifest_path)?;
                    let revisions =
                        smithy_rs.rev_list("HEAD", &manifest.smithy_rs_revision, None)?;
                    Ok(self
                        .aws_sdk_rust
                        .into_iter()
                        .filter(|entry| match entry {
                            ChangelogEntry::AwsSdkModel(_) => true,
                            ChangelogEntry::HandAuthored(entry) => {
                                if let Some(since_commit) = &entry.since_commit {
                                    revisions.iter().any(|rev| rev.as_ref() == since_commit)
                                } else {
                                    true
                                }
                            }
                        })
                        .collect())
                } else {
                    if self.aws_sdk_rust.iter().any(|entry| {
                        entry.hand_authored().map(|e| e.since_commit.is_some()) == Some(true)
                    }) {
                        bail!("SDK changelog entries have `since_commit` information, but no previous release versions manifest was given");
                    }
                    Ok(self.aws_sdk_rust)
                }
            }
            ChangeSet::SmithyRs => self.filter_smithy_rs_sdk_type(smithy_rs_sdk)
        }
    }

    fn filter_smithy_rs_sdk_type(self, smithy_rs_sdk : Option<SdkAffected>) -> Result<Vec<ChangelogEntry>> {
        let sdk_to_include = if let Some(sdk_query) = smithy_rs_sdk {
            sdk_query
        }
        else {
            SdkAffected::Both
        };

        if sdk_to_include == SdkAffected::Both {
            Ok(self.smithy_rs)
        }
        else {
            let result = self.smithy_rs
                .into_iter()
                .filter(|entry| {
                    match entry {
                        ChangelogEntry::HandAuthored(hand_entry) => {
                            if let Some(change_affects) = hand_entry.meta.sdk {
                                sdk_to_include == change_affects || change_affects == SdkAffected::Both
                            }
                            else {
                                // a missing sdk entry in the meta is considered a client
                                sdk_to_include == SdkAffected::Client
                            }
                        },
                        _ => true,
                    }
                })
                .collect();
            Ok(result)
        }
    }
}

impl From<Changelog> for ChangelogEntries {
    fn from(mut changelog: Changelog) -> Self {
        changelog.aws_sdk_rust.sort_by_key(|entry| !entry.meta.tada);
        changelog.sdk_models.sort_by(|a, b| a.module.cmp(&b.module));
        changelog.smithy_rs.sort_by_key(|entry| !entry.meta.tada);

        ChangelogEntries {
            smithy_rs: changelog
                .smithy_rs
                .into_iter()
                .map(ChangelogEntry::HandAuthored)
                .collect(),
            aws_sdk_rust: changelog
                .aws_sdk_rust
                .into_iter()
                .map(ChangelogEntry::HandAuthored)
                .chain(
                    changelog
                        .sdk_models
                        .into_iter()
                        .map(ChangelogEntry::AwsSdkModel),
                )
                .collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum ChangelogEntry {
    HandAuthored(HandAuthoredEntry),
    AwsSdkModel(SdkModelEntry),
}

impl ChangelogEntry {
    pub fn hand_authored(&self) -> Option<&HandAuthoredEntry> {
        match self {
            ChangelogEntry::HandAuthored(hand_authored) => Some(hand_authored),
            _ => None,
        }
    }

    pub fn aws_sdk_model(&self) -> Option<&SdkModelEntry> {
        match self {
            ChangelogEntry::AwsSdkModel(sdk_model) => Some(sdk_model),
            _ => None,
        }
    }
}
