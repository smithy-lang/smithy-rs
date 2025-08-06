/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

pub(crate) mod build_connector {
    use hyper_util::client::legacy as client;
    use client::connect::HttpConnector;
    use s2n_tls::security::Policy;
    use crate::tls::TlsContext;
    use std::sync::LazyLock;

    // Default S2N security policy which sets protocol versions and cipher suites
    //  See https://aws.github.io/s2n-tls/usage-guide/ch06-security-policies.html
    const S2N_POLICY_VERSION: &str = "20230317";

    fn base_config() -> s2n_tls::config::Builder {
        let mut builder = s2n_tls::config::Config::builder();
        let policy = Policy::from_version(S2N_POLICY_VERSION).unwrap();
        builder.set_security_policy(&policy).expect("valid s2n security policy");
        // default is true
        builder.with_system_certs(false).unwrap();
        builder
    }

    static CACHED_CONFIG: LazyLock<s2n_tls::config::Config> = LazyLock::new(|| {
        let mut config = base_config();
        config.with_system_certs(true).unwrap();
        // actually loads the system certs
        config.build().expect("valid s2n config")
    });

    impl TlsContext {
        fn s2n_config(&self) -> s2n_tls::config::Config {
            // TODO(s2n-tls): s2n does not support turning a config back into a builder or a way to load a trust store and re-use it
            // instead if we are only using the defaults then use a cached config, otherwise pay the cost to build a new one
            if self.trust_store.enable_native_roots && self.trust_store.custom_certs.is_empty() {
                CACHED_CONFIG.clone()
            } else {
                let mut config = base_config();
                config.with_system_certs(self.trust_store.enable_native_roots).unwrap();
                for pem_cert in &self.trust_store.custom_certs {
                    config.trust_pem(pem_cert.0.as_slice()).expect("valid certificate");
                }
                config.build().expect("valid s2n config")
            }
        }
    }

    pub(crate) fn wrap_connector<R>(
        mut http_connector: HttpConnector<R>,
        tls_context: &TlsContext,
    ) -> s2n_tls_hyper::connector::HttpsConnector<HttpConnector<R>> {
        let config = tls_context.s2n_config();
        http_connector.enforce_http(false);
        let mut builder = s2n_tls_hyper::connector::HttpsConnector::builder_with_http(http_connector, config);
        builder.with_plaintext_http(true);
        builder.build()
    }
}
