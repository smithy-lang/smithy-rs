/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Post-publish build canary for smithy-rs.
//!
//! Every check this repository runs today builds the workspace *before* publish. This crate covers
//! what happens after: each release of the smithy runtime crates is checked by resolving and
//! building a set of manifests a consumer would plausibly write, against the published crates
//! on crates.io, across five compiler versions, for 24 hours — and a failure pages, with a verdict
//! naming which part broke.
//!
//! The scenario table, the leg matrix, manifest rendering, the leg runner and the verdict
//! classifier need no credential, so all of that can be exercised on a laptop. Two modules are the
//! exception: [`metric`] shells out to the AWS CLI and [`watch`] to `gh`. The alarm itself lives
//! outside this crate.

pub mod baseline;
pub mod classify;
pub mod floor;
pub mod leg;
pub mod lockfile;
pub mod manifest;
pub mod metric;
pub mod publish;
pub mod scenarios;
pub mod summary;
pub mod test_registry;
pub mod toolchain;
pub mod watch;

#[cfg(test)]
mod test_support;
