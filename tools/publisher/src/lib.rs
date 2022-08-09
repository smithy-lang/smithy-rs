/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

pub const SDK_REPO_CRATE_PATH: &str = "sdk";
pub const SDK_REPO_NAME: &str = "aws-sdk-rust";
pub const SMITHYRS_REPO_NAME: &str = "smithy-rs";

// https://github.com/orgs/awslabs/teams/smithy-rs-server
pub const SMITHY_RS_SERVER_OWNER: &str = "github:awslabs:smithy-rs-server";

// https://github.com/orgs/awslabs/teams/rust-sdk-owners
pub const RUST_SDK_OWNER: &str = "github:awslabs:rust-sdk-owners";

// https://github.com/aws-sdk-rust-ci
pub const RUST_SDK_CI_OWNER: &str = "aws-sdk-rust-ci";

pub mod cargo;
pub mod fs;
pub mod package;
pub mod retry;
pub mod sort;
pub mod subcommand;
