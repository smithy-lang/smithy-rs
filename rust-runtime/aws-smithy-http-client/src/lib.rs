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
//! - `default-tls`: Enable default TLS provider (used by `default_client()` to provide a default configured HTTPS client)
//! - `hyper-014`: (Deprecated) HTTP client implementation based on hyper-0.14.x.
//! - `test-util`: Enables utilities for unit tests. DO NOT ENABLE IN PRODUCTION.

#![warn(
    missing_docs,
    rustdoc::missing_crate_level_docs,
    unreachable_pub,
    rust_2018_idioms
)]
#![cfg_attr(docsrs, feature(doc_cfg))]

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

/// Default HTTP and TLS connectors
#[cfg(feature = "hyper-1")]
pub(crate) mod client;
#[cfg(feature = "hyper-1")]
pub use client::{default_client, default_connector, tls, Builder, Connector, ConnectorBuilder};

#[cfg(feature = "test-util")]
pub mod test_util;

#[allow(unused_macros, unused_imports)]
#[macro_use]
pub(crate) mod cfg {
    /// Any TLS provider enabled
    macro_rules! cfg_tls {
        ($($item:item)*) => {
            $(
                #[cfg(any(
                    feature = "rustls-aws-lc",
                    feature = "rustls-aws-lc-fips",
                    feature = "rustls-ring"
                ))]
                #[cfg_attr(docsrs, doc(cfg(any(feature = "rustls-aws-lc", feature = "rustls-aws-lc-fips", feature = "rustls-ring"))))]
                $item
            )*
        }
    }

    /// Any rustls provider enabled
    macro_rules! cfg_rustls {
        ($($item:item)*) => {
            $(
                #[cfg(any(
                    feature = "rustls-aws-lc",
                    feature = "rustls-aws-lc-fips",
                    feature = "rustls-ring"
                ))]
                #[cfg_attr(docsrs, doc(cfg(any(feature = "rustls-aws-lc", feature = "rustls-aws-lc-fips", feature = "rustls-ring"))))]
                $item
            )*
        }
    }
    pub(crate) use cfg_rustls;
    pub(crate) use cfg_tls;
}
