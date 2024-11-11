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
//! - `hyper-014`: (Deprecated) HTTP client implementation based on hyper-0.14.x.
//! - `test-util`: Enables utilities for unit tests. DO NOT ENABLE IN PRODUCTION.

#![warn(
    missing_docs,
    rustdoc::missing_crate_level_docs,
    unreachable_pub,
    rust_2018_idioms
)]

// ideally hyper_014 would just be exposed as is but due to
// https://github.com/rust-lang/rust/issues/47238 we get clippy warnings we can't suppress
#[cfg(feature = "hyper-014")]
pub(crate) mod hyper_legacy;

/// Legacy HTTP and TLS connectors that use hyper 0.14.x and rustls.
#[cfg(feature = "hyper-014")]
#[deprecated = "hyper 0.14.x support is deprecated, please migrate to 1.x client"]
pub mod hyper_014 {
    pub use crate::hyper_legacy::*;
}

// FIXME - remove hyper-1 from the name

/// Default HTTP and TLS connectors
#[cfg(feature = "hyper-1")]
pub(crate) mod client;
#[cfg(feature = "hyper-1")]
pub use client::{Builder, Connector, ConnectorBuilder, CryptoMode};

#[cfg(feature = "test-util")]
pub mod test_util;
