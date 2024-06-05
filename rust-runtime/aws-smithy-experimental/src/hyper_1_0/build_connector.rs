/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![allow(unused_imports)]

use crate::hyper_1_0::{HyperUtilResolver, Inner};
use aws_smithy_runtime_api::client::dns::ResolveDns;
use client::connect::HttpConnector;
use hyper_rustls::HttpsConnector;
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

pub(crate) fn make_tls<R>(
    resolver: R,
    crypto_provider: CryptoProvider,
) -> HttpsConnector<HttpConnector<R>> {
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

pub(super) fn https_with_resolver<R: ResolveDns>(
    crypto_provider: Inner,
    resolver: R,
) -> HttpsConnector<HttpConnector<HyperUtilResolver<R>>> {
    make_tls(HyperUtilResolver { resolver }, crypto_provider.provider())
}
