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
#![cfg_attr(docsrs, feature(doc_cfg))]

use aws_smithy_runtime_api::client::behavior_version::BehaviorVersion;
use aws_smithy_runtime_api::client::http::SharedHttpClient;

// NOTE: We created default client options to evolve defaults over time (e.g. allow passing a different DNS resolver)
/// Configuration options for the default HTTPS client
#[derive(Debug, Clone)]
pub struct DefaultClientOptions {
    _behavior_version: BehaviorVersion,
}

impl Default for DefaultClientOptions {
    fn default() -> Self {
        DefaultClientOptions {
            _behavior_version: BehaviorVersion::latest(),
        }
    }
}

impl DefaultClientOptions {
    /// Set the behavior version to use
    #[allow(unused)]
    pub fn with_behavior_version(mut self, behavior_version: BehaviorVersion) -> Self {
        self._behavior_version = behavior_version;
        self
    }
}

#[cfg(unix)]
/// Creates an HTTPS client using the default TLS provider
pub fn default_https_client(_options: DefaultClientOptions) -> Option<SharedHttpClient> {
    use aws_smithy_http_client::{tls, Builder};
    tracing::trace!("creating a new default hyper 1.x client using s2n-tls");
    Some(
        Builder::new()
            .tls_provider(tls::Provider::S2nTls)
            .build_https(),
    )
}

#[cfg(windows)]
/// Creates an HTTPS client using the default TLS provider
pub fn default_https_client(_options: DefaultClientOptions) -> Option<SharedHttpClient> {
    use aws_smithy_http_client::{tls, Builder};
    tracing::trace!("creating a new default hyper 1.x client using rustls<aws-lc>");
    Some(
        Builder::new()
            .tls_provider(tls::Provider::Rustls(
                tls::rustls_provider::CryptoMode::AwsLc,
            ))
            .build_https(),
    )
}
