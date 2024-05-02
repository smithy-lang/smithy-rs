/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

pub mod changelog;
pub mod ci;
pub mod command;
pub mod git;
#[macro_use]
pub mod macros;
pub mod index;
pub mod package;
pub mod release_tag;
pub mod retry;
pub mod shell;
pub mod versions_manifest;

// https://github.com/aws-sdk-rust-ci
pub const RUST_SDK_CI_OWNER: &str = "aws-sdk-rust-ci";
