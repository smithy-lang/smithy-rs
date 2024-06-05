/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::hyper_1_0::build_connector::make_tls;
use crate::hyper_1_0::{CryptoMode, Inner};
use client::connect::HttpConnector;
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy as client;
use hyper_util::client::legacy::connect::dns::GaiResolver;

#[cfg(feature = "crypto-ring")]
pub(crate) static HTTPS_NATIVE_ROOTS_RING: once_cell::sync::Lazy<HttpsConnector<HttpConnector>> =
    once_cell::sync::Lazy::new(|| make_tls(GaiResolver::new(), CryptoMode::Ring.provider()));

#[cfg(feature = "crypto-aws-lc")]
pub(crate) static HTTPS_NATIVE_ROOTS_AWS_LC: once_cell::sync::Lazy<HttpsConnector<HttpConnector>> =
    once_cell::sync::Lazy::new(|| make_tls(GaiResolver::new(), CryptoMode::AwsLc.provider()));

#[cfg(feature = "crypto-aws-lc-fips")]
pub(crate) static HTTPS_NATIVE_ROOTS_AWS_LC_FIPS: once_cell::sync::Lazy<
    HttpsConnector<HttpConnector>,
> = once_cell::sync::Lazy::new(|| make_tls(GaiResolver::new(), CryptoMode::AwsLcFips.provider()));

pub(super) fn cached_https(mode: Inner) -> hyper_rustls::HttpsConnector<HttpConnector> {
    match mode {
        #[cfg(feature = "crypto-ring")]
        Inner::Standard(CryptoMode::Ring) => HTTPS_NATIVE_ROOTS_RING.clone(),
        #[cfg(feature = "crypto-aws-lc")]
        Inner::Standard(CryptoMode::AwsLc) => HTTPS_NATIVE_ROOTS_AWS_LC.clone(),
        #[cfg(feature = "crypto-aws-lc-fips")]
        Inner::Standard(CryptoMode::AwsLcFips) => HTTPS_NATIVE_ROOTS_AWS_LC_FIPS.clone(),
        #[allow(unreachable_patterns)]
        Inner::Standard(_) => unreachable!("unexpected mode"),
        Inner::Custom(provider) => make_tls(GaiResolver::new(), provider),
    }
}
