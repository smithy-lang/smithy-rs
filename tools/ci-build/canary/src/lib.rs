/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Post-publish build canary for smithy-rs.
//!
//! Design: `.kiro/specs/build-canary/`. The short version is that every release of the smithy
//! runtime crates is checked by resolving and building twelve manifests that a consumer would
//! plausibly write, across five compiler versions, for 24 hours — and that a failure pages, with a
//! verdict naming which part broke.
//!
//! This crate is the offline half: it owns the scenario table, the leg matrix, manifest rendering,
//! the leg runner and the verdict classifier. Nothing here touches AWS or needs a credential, which
//! is what makes Stage A of the rollout testable on a laptop.

pub mod classify;
pub mod leg;
pub mod manifest;
pub mod scenarios;
pub mod toolchain;
