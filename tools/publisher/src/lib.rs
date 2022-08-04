/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

pub const SDK_REPO_CRATE_PATH: &str = "sdk";
pub const SDK_REPO_NAME: &str = "aws-sdk-rust";
pub const SMITHYRS_REPO_NAME: &str = "smithy-rs";

// Crate ownership for SDK crates. Crates.io requires that at least one owner
// is an individual rather than a team, so we use the automation user for that.
pub const CRATE_OWNERS: &[&str] = &[
    // https://github.com/orgs/awslabs/teams/rust-sdk-owners
    "github:awslabs:rust-sdk-owners",
    // https://github.com/orgs/awslabs/teams/smithy-rs-server
    "github:awslabs:smithy-rs-server",
    // https://github.com/aws-sdk-rust-ci
    "aws-sdk-rust-ci",
];

pub mod cargo;
pub mod fs;
pub mod package;
pub mod retry;
pub mod sort;
pub mod subcommand;
