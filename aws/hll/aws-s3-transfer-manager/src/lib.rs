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
    // TODO - re-enable missing_docs,
    rustdoc::missing_crate_level_docs,
    unreachable_pub,
    rust_2018_idioms
)]

pub(crate) const MEBI_BYTE: u64 = 1024 * 1024;
pub(crate) const GIBI_BYTE: u64 = MEBI_BYTE * 1024;
pub(crate) const MIN_PART_SIZE: u64 = 5 * MEBI_BYTE;

pub mod download;
pub mod error;
