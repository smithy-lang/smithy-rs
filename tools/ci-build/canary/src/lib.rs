/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Post-publish build canary for smithy-rs.
//!
//! Every check this repository runs today builds the workspace *before* publish. This crate covers
//! what happens after: each release of the smithy runtime crates is checked by resolving and
//! building thirteen manifests that a consumer would plausibly write, against the published crates
//! on crates.io, across five compiler versions, for 24 hours — and a failure pages, with a verdict
//! naming which part broke.
//!
//! This crate is the offline half: it owns the scenario table, the leg matrix, manifest rendering,
//! the leg runner and the verdict classifier. Nothing here touches AWS or needs a credential, so all
//! of it can be exercised on a laptop. Reporting a verdict as a metric, and alarming on it, live
//! outside this crate.

pub mod classify;
pub mod leg;
pub mod manifest;
pub mod publish;
pub mod scenarios;
pub mod toolchain;
