/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! AWS Access Tokens for SSO
//!
//! When authenticating with an AWS Builder ID, single sign-on (SSO) will provide
//! an access token that can then be used to authenticate with services such as
//! Code Catalyst.
//!
//! This module provides the [`ProvideAccessToken`] trait that is used to configure
//! token providers in the SDK config.

use crate::{provider::error::AccessTokenError, provider::future, AccessToken};
use std::sync::Arc;

/// Result type for token providers
pub type Result = std::result::Result<AccessToken, AccessTokenError>;

/// Access Token Provider
pub trait ProvideAccessToken: Send + Sync + std::fmt::Debug {
    /// Returns a future that provides an access token.
    fn provide_access_token<'a>(&'a self) -> future::ProvideAccessToken<'a>
    where
        Self: 'a;
}

/// Access token provider wrapper that may be shared.
///
/// Newtype wrapper around [`ProvideAccessToken`] that implements `Clone` using an internal `Arc`.
#[derive(Clone, Debug)]
pub struct SharedAccessTokenProvider(Arc<dyn ProvideAccessToken>);

impl SharedAccessTokenProvider {
    /// Create a new [`SharedAccessTokenProvider`] from [`ProvideAccessToken`].
    ///
    /// The given provider will be wrapped in an internal `Arc`. If your
    /// provider is already in an `Arc`, use `SharedAccessTokenProvider::from(provider)` instead.
    pub fn new(provider: impl ProvideAccessToken + 'static) -> Self {
        Self(Arc::new(provider))
    }
}

impl AsRef<dyn ProvideAccessToken> for SharedAccessTokenProvider {
    fn as_ref(&self) -> &(dyn ProvideAccessToken + 'static) {
        self.0.as_ref()
    }
}

impl From<Arc<dyn ProvideAccessToken>> for SharedAccessTokenProvider {
    fn from(provider: Arc<dyn ProvideAccessToken>) -> Self {
        SharedAccessTokenProvider(provider)
    }
}

impl ProvideAccessToken for SharedAccessTokenProvider {
    fn provide_access_token<'a>(&'a self) -> future::ProvideAccessToken<'a>
    where
        Self: 'a,
    {
        self.0.provide_access_token()
    }
}
