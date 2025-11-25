/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/* Automatically managed default lints */
#![cfg_attr(docsrs, feature(doc_cfg))]
/* End of automatically managed default lints */
//! CloudFront URL and cookie signing utilities for AWS SDK.

#![warn(
    missing_docs,
    rustdoc::missing_crate_level_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

/// Error types for CloudFront signing operations.
pub mod error;
mod key;
mod policy;
mod sign;

pub use key::PrivateKey;
pub use sign::{SignedCookies, SignedUrl, SigningRequest, SigningRequestBuilder};

/// Sign a CloudFront URL with canned or custom policy
pub fn sign_url(request: SigningRequest) -> Result<SignedUrl, error::SigningError> {
    request.sign_url()
}

/// Generate signed cookies with canned or custom policy
pub fn sign_cookies(request: SigningRequest) -> Result<SignedCookies, error::SigningError> {
    request.sign_cookies()
}
