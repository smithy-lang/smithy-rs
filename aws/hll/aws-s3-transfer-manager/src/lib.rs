/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/* Automatically managed default lints */
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
/* End of automatically managed default lints */

//! AWS S3 Transfer Manager
//!
//! # Crate Features
//!
//! - `test-util`: Enables utilities for unit tests. DO NOT ENABLE IN PRODUCTION.

#![warn(
    missing_debug_implementations,
    missing_docs,
    rustdoc::missing_crate_level_docs,
    unreachable_pub,
    rust_2018_idioms
)]

pub(crate) const MEBIBYTE: u64 = 1024 * 1024;
/// Abstractions for downloading objects from S3
pub mod download;

/// Error types emitted by `aws-s3-transfer-manager`
pub mod error;

/// Types and helpers for I/O
pub mod io;

/// Abstractions for downloading objects from Amazon S3
pub mod upload;
