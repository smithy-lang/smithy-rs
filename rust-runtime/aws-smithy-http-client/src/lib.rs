/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/* Automatically managed default lints */
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
/* End of automatically managed default lints */

//! HTTP client implementation for smithy-rs generated code.
//!
//! # Crate Features
//!
//! - `test-util`: Enables utilities for unit tests. DO NOT ENABLE IN PRODUCTION.

#![warn(
    missing_docs,
    rustdoc::missing_crate_level_docs,
    unreachable_pub,
    rust_2018_idioms
)]

/// Default HTTP and TLS connectors that use hyper 0.14.x and rustls.
#[deprecated = "hyper 0.14.x support is deprecated, please migrate to 1.x client"]
pub mod hyper_014;
