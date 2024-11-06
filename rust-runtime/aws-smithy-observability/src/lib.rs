/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/* Automatically managed default lints */
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
/* End of automatically managed default lints */
#![warn(
    missing_docs,
    rustdoc::missing_crate_level_docs,
    unreachable_pub,
    rust_2018_idioms
)]

//! Smithy Observability
//TODO(smithyobservability): once we have finalized everything and integrated metrics with our runtime
// libraries update this with detailed usage docs and examples

pub mod attributes;
pub mod error;
pub mod global;
pub mod meter;
mod noop;
pub mod provider;
