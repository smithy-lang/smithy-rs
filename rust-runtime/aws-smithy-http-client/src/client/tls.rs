/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/// Choice of underlying cryptography library
#[derive(Debug, Eq, PartialEq, Clone)]
#[non_exhaustive]
pub enum Provider {
    #[cfg(any(
        feature = "rustls-aws-lc",
        feature = "rustls-aws-lc-fips",
        feature = "rustls-ring"
    ))]
    /// TLS provider based on [rustls](https://github.com/rustls/rustls)
    Rustls(rustls_provider::CryptoMode),
    // TLS provider based on [S2N](https://github.com/aws/s2n-tls)
    // S2n,
    // TODO(hyper1) s2n support

    // TODO(hyper1): consider native-tls support?
}

/// rustls based support and adapters
#[cfg(any(
    feature = "rustls-aws-lc",
    feature = "rustls-aws-lc-fips",
    feature = "rustls-ring"
))]
pub mod rustls_provider {
    use crate::client::tls::Provider;
    use rustls::crypto::CryptoProvider;

    /// Choice of underlying cryptography library (this only applies to rustls)
    #[derive(Debug, Eq, PartialEq, Clone)]
    #[non_exhaustive]
    pub enum CryptoMode {
        /// Crypto based on [ring](https://github.com/briansmith/ring)
        #[cfg(feature = "rustls-ring")]
        Ring,
        /// Crypto based on [aws-lc](https://github.com/aws/aws-lc-rs)
        #[cfg(feature = "rustls-aws-lc")]
        AwsLc,
        /// FIPS compliant variant of [aws-lc](https://github.com/aws/aws-lc-rs)
        #[cfg(feature = "rustls-aws-lc-fips")]
        AwsLcFips,
    }

    impl CryptoMode {
        fn provider(self) -> CryptoProvider {
            match self {
                #[cfg(feature = "rustls-aws-lc")]
                CryptoMode::AwsLc => rustls::crypto::aws_lc_rs::default_provider(),

                #[cfg(feature = "rustls-ring")]
                CryptoMode::Ring => rustls::crypto::ring::default_provider(),

                #[cfg(feature = "rustls-aws-lc-fips")]
                CryptoMode::AwsLcFips => {
                    let provider = rustls::crypto::default_fips_provider();
                    assert!(
                        provider.fips(),
                        "FIPS was requested but the provider did not support FIPS"
                    );
                    provider
                }
            }
        }
    }

    impl Provider {
        /// Create a TLS provider based on [rustls](https://github.com/rustls/rustls)
        /// and the given [`CryptoMode`]
        pub fn rustls(mode: CryptoMode) -> Provider {
            Provider::Rustls(mode)
        }
    }

    #[allow(unused_imports)]
    pub(crate) mod cached_connectors {
        use client::connect::HttpConnector;
        use hyper_util::client::legacy as client;
        use hyper_util::client::legacy::connect::dns::GaiResolver;

        use super::build_connector::make_tls;
        use super::CryptoMode;

        #[cfg(feature = "rustls-ring")]
        static HTTPS_NATIVE_ROOTS_RING: once_cell::sync::Lazy<
            hyper_rustls::HttpsConnector<HttpConnector>,
        > = once_cell::sync::Lazy::new(|| {
            make_tls(GaiResolver::new(), CryptoMode::Ring.provider())
        });

        #[cfg(feature = "rustls-aws-lc")]
        static HTTPS_NATIVE_ROOTS_AWS_LC: once_cell::sync::Lazy<
            hyper_rustls::HttpsConnector<HttpConnector>,
        > = once_cell::sync::Lazy::new(|| {
            make_tls(GaiResolver::new(), CryptoMode::AwsLc.provider())
        });

        #[cfg(feature = "rustls-aws-lc-fips")]
        static HTTPS_NATIVE_ROOTS_AWS_LC_FIPS: once_cell::sync::Lazy<
            hyper_rustls::HttpsConnector<HttpConnector>,
        > = once_cell::sync::Lazy::new(|| {
            make_tls(GaiResolver::new(), CryptoMode::AwsLcFips.provider())
        });

        pub(crate) fn cached_https(
            mode: CryptoMode,
        ) -> hyper_rustls::HttpsConnector<HttpConnector> {
            match mode {
                #[cfg(feature = "rustls-ring")]
                CryptoMode::Ring => HTTPS_NATIVE_ROOTS_RING.clone(),
                #[cfg(feature = "rustls-aws-lc")]
                CryptoMode::AwsLc => HTTPS_NATIVE_ROOTS_AWS_LC.clone(),
                #[cfg(feature = "rustls-aws-lc-fips")]
                CryptoMode::AwsLcFips => HTTPS_NATIVE_ROOTS_AWS_LC_FIPS.clone(),
            }
        }
    }

    pub(crate) mod build_connector {
        use crate::client::dns::HyperUtilResolver;
        use crate::client::tls::rustls_provider::CryptoMode;
        use aws_smithy_runtime_api::client::dns::ResolveDns;
        use client::connect::HttpConnector;
        use hyper_util::client::legacy as client;
        use rustls::crypto::CryptoProvider;
        use std::sync::Arc;

        fn restrict_ciphers(base: CryptoProvider) -> CryptoProvider {
            let suites = &[
                rustls::CipherSuite::TLS13_AES_256_GCM_SHA384,
                rustls::CipherSuite::TLS13_AES_128_GCM_SHA256,
                // TLS1.2 suites
                rustls::CipherSuite::TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
                rustls::CipherSuite::TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
                rustls::CipherSuite::TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
                rustls::CipherSuite::TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
                rustls::CipherSuite::TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256,
            ];
            let supported_suites = suites
                .iter()
                .flat_map(|suite| {
                    base.cipher_suites
                        .iter()
                        .find(|s| &s.suite() == suite)
                        .cloned()
                })
                .collect::<Vec<_>>();
            CryptoProvider {
                cipher_suites: supported_suites,
                ..base
            }
        }

        pub(super) fn make_tls<R>(
            resolver: R,
            crypto_provider: CryptoProvider,
        ) -> hyper_rustls::HttpsConnector<HttpConnector<R>> {
            use hyper_rustls::ConfigBuilderExt;
            let mut base_connector = HttpConnector::new_with_resolver(resolver);
            base_connector.enforce_http(false);
            hyper_rustls::HttpsConnectorBuilder::new()
                .with_tls_config(
                    rustls::ClientConfig::builder_with_provider(Arc::new(restrict_ciphers(crypto_provider)))
                        .with_safe_default_protocol_versions()
                        .expect("Error with the TLS configuration. Please file a bug report under https://github.com/smithy-lang/smithy-rs/issues.")
                        .with_native_roots().expect("error with TLS configuration.")
                        .with_no_client_auth()
                )
                .https_or_http()
                .enable_http1()
                .enable_http2()
                .wrap_connector(base_connector)
        }

        pub(crate) fn https_with_resolver<R: ResolveDns>(
            crypto_mode: CryptoMode,
            resolver: R,
        ) -> hyper_rustls::HttpsConnector<HttpConnector<HyperUtilResolver<R>>> {
            make_tls(HyperUtilResolver { resolver }, crypto_mode.provider())
        }
    }
}
