/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use crate::cfg::{cfg_rustls, cfg_s2n_tls};

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
    /// TLS provider based on [s2n-tls](https://github.com/aws/s2n-tls)
    #[cfg(feature = "s2n-tls")]
    S2nTls,
}

// TODO - add local test that should fail now for a custom ca bundle (maybe test with override to see it _can_ work)
// TODO - dup hyper-rustls native cert handling and don't use `with_native_certs`
// TODO - replace all the client caching with simply caching native certs (with replaced logic)

#[derive(Debug)]
pub struct TlsContext {
    // TODO - should we support AWS_CA_BUNDLE?
}

impl TlsContext {
    pub fn new() -> Self {
        Self {}
    }

    // fn with_root_ca(pem_bytes: &[u8]) {
    //
    // }
}

cfg_rustls! {
    /// rustls based support and adapters
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
                make_tls(GaiResolver::new(), CryptoMode::Ring)
            });

            #[cfg(feature = "rustls-aws-lc")]
            static HTTPS_NATIVE_ROOTS_AWS_LC: once_cell::sync::Lazy<
                hyper_rustls::HttpsConnector<HttpConnector>,
            > = once_cell::sync::Lazy::new(|| {
                make_tls(GaiResolver::new(), CryptoMode::AwsLc)
            });

            #[cfg(feature = "rustls-aws-lc-fips")]
            static HTTPS_NATIVE_ROOTS_AWS_LC_FIPS: once_cell::sync::Lazy<
                hyper_rustls::HttpsConnector<HttpConnector>,
            > = once_cell::sync::Lazy::new(|| {
                make_tls(GaiResolver::new(), CryptoMode::AwsLcFips)
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
            use crate::client::tls::rustls_provider::CryptoMode;
            use hyper_util::client::legacy as client;
            use client::connect::HttpConnector;
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
                crypto_mode: CryptoMode,
            ) -> hyper_rustls::HttpsConnector<HttpConnector<R>> {
                // use the base connector through our `Connector` type to ensure defaults are consistent
                let base_connector = crate::client::Connector::builder()
                    .base_connector_with_resolver(resolver);
                wrap_connector(base_connector, crypto_mode)
            }

            pub(crate) fn wrap_connector<R>(
                mut conn: HttpConnector<R>,
                crypto_mode: CryptoMode,
            ) -> hyper_rustls::HttpsConnector<HttpConnector<R>> {
                use hyper_rustls::ConfigBuilderExt;
                conn.enforce_http(false);
                hyper_rustls::HttpsConnectorBuilder::new()
                    .with_tls_config(
                        rustls::ClientConfig::builder_with_provider(Arc::new(restrict_ciphers(crypto_mode.provider())))
                            .with_safe_default_protocol_versions()
                            .expect("Error with the TLS configuration. Please file a bug report under https://github.com/smithy-lang/smithy-rs/issues.")
                            .with_native_roots().expect("error with TLS configuration.")
                            .with_no_client_auth()
                    )
                    .https_or_http()
                    .enable_http1()
                    .enable_http2()
                    .wrap_connector(conn)
            }
        }
    }
}

cfg_s2n_tls! {
    /// s2n-tls based support and adapters
    pub(crate) mod s2n_tls_provider {
        pub(crate) mod cached_connectors {
            use hyper_util::client::legacy as client;
            use client::connect::HttpConnector;
            use hyper_util::client::legacy::connect::dns::GaiResolver;
            use super::build_connector::make_tls;

            static CACHED_CONNECTOR: once_cell::sync::Lazy<
                s2n_tls_hyper::connector::HttpsConnector<HttpConnector>,
            > = once_cell::sync::Lazy::new(|| {
                make_tls(GaiResolver::new())
            });

            pub(crate) fn cached_https() -> s2n_tls_hyper::connector::HttpsConnector<HttpConnector> {
                CACHED_CONNECTOR.clone()
            }
        }

        pub(crate) mod build_connector {
            use hyper_util::client::legacy as client;
            use client::connect::HttpConnector;
            use s2n_tls::security::Policy;

            /// Default S2N security policy which sets protocol versions and cipher suites
            ///  See https://aws.github.io/s2n-tls/usage-guide/ch06-security-policies.html
            const S2N_POLICY_VERSION: &'static str = "20230317";

            pub(super) fn make_tls<R>(
                resolver: R,
            ) -> s2n_tls_hyper::connector::HttpsConnector<HttpConnector<R>> {
                // use the base connector through our `Connector` type to ensure defaults are consistent
                let base_connector = crate::client::Connector::builder()
                    .base_connector_with_resolver(resolver);
                wrap_connector(base_connector)
            }

            pub(crate) fn wrap_connector<R>(
                mut http_connector: HttpConnector<R>,
            ) -> s2n_tls_hyper::connector::HttpsConnector<HttpConnector<R>> {
                http_connector.enforce_http(false);
                let config = {
                    let mut builder = s2n_tls::config::Config::builder();
                    let policy = Policy::from_version(S2N_POLICY_VERSION).unwrap();
                    builder.set_security_policy(&policy).unwrap();
                    builder.build().unwrap()
                };
                let mut builder = s2n_tls_hyper::connector::HttpsConnector::builder_with_http(http_connector, config);
                builder.with_plaintext_http(true);
                builder.build()
            }
        }
    }
}
