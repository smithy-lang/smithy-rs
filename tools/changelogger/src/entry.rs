/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use clap::clap_derive::ArgEnum;
use smithy_rs_tool_common::changelog::{Changelog, HandAuthoredEntry, SdkModelEntry};

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
    pub fn for_change_set(&self, change_set: ChangeSet) -> &[ChangelogEntry] {
        match change_set {
            ChangeSet::AwsSdk => &self.aws_sdk_rust,
            ChangeSet::SmithyRs => &self.smithy_rs,
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
