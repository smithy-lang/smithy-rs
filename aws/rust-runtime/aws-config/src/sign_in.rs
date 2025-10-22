/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Credentials from an AWS Console session vended by AWS Sign-In.

mod cache;
mod token;

use crate::provider_config::ProviderConfig;
use crate::sign_in::cache::{load_cached_token, SignInTokenError};
use crate::sign_in::token::SignInToken;
use aws_credential_types::provider::future;
use aws_credential_types::provider::ProvideCredentials;
use aws_smithy_async::time::SharedTimeSource;
use aws_smithy_runtime::expiring_cache::ExpiringCache;
use aws_types::os_shim_internal::{Env, Fs};
use aws_types::SdkConfig;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::SystemTime;
// TODO(sign-in): fill in additional details on this provider, examples, and links to documentation

const REFRESH_BUFFER_TIME: Duration = Duration::from_secs(5 * 60 /* 5 minutes */);
const MIN_TIME_BETWEEN_REFRESH: Duration = Duration::from_secs(30);

/// AWS credentials provider vended by AWS Sign-In. This provider allows users to acquire AWS
/// credentials that correspond to an AWS Console session.
#[derive(Debug)]
pub struct SignInCredentialProvider {
    inner: Arc<Inner>,
    token_cache: ExpiringCache<SignInToken, SignInTokenError>,
}

#[derive(Debug)]
struct Inner {
    fs: Fs,
    env: Env,
    session_arn: String,
    sdk_config: SdkConfig,
    time_source: SharedTimeSource,
    last_refresh_attempt: Mutex<Option<SystemTime>>,
}

impl SignInCredentialProvider {
    /// Create a new [`Builder`] for the given login session ARN.
    ///
    /// The `session_arn` argument should take the form an Amazon Resource Name (ARN) like
    ///
    /// ```text
    /// arn:aws:iam::0123456789012:user/Admin
    /// ```
    pub fn builder(session_arn: impl Into<String>) -> Builder {
        Builder {
            session_arn: session_arn.into(),
            provider_config: None,
        }
    }

    async fn resolve_token(
        &self,
        time_source: SharedTimeSource,
    ) -> Result<SignInToken, SignInTokenError> {
        let token_cache = self.token_cache.clone();
        if let Some(token) = token_cache
            .yield_or_clear_if_expired(time_source.now())
            .await
        {
            tracing::debug!("using cached Sign-In token");
            return Ok(token);
        }

        let inner = self.inner.clone();
        let token = token_cache
            .get_or_load(|| async move {
                tracing::debug!("expiring cache asked for an updated Sign-In token");
                let mut token =
                    load_cached_token(&inner.env, &inner.fs, &inner.session_arn).await?;

                tracing::debug!("loaded cached Sign-In token");

                let now = time_source.now();
                let expired = token.expires_at() <= now;
                let expires_soon = token.expires_at() - REFRESH_BUFFER_TIME <= now;
                let last_refresh = *inner.last_refresh_attempt.lock().unwrap();
                let min_time_passed = last_refresh
                    .map(|lr| {
                        now.duration_since(lr).expect("last_refresh is in the past")
                            >= MIN_TIME_BETWEEN_REFRESH
                    })
                    .unwrap_or(true);

                let refreshable = min_time_passed;

                tracing::debug!(
                    expired = ?expired,
                    expires_soon = ?expires_soon,
                    min_time_passed = ?min_time_passed,
                    refreshable = ?refreshable,
                    will_refresh = ?(expires_soon && refreshable),
                    "cached Sign-In token refresh decision"
                );

                // Fail fast if the token has expired and we can't refresh it
                if expired && !refreshable {
                    tracing::debug!("cached Sign-In token is expired and cannot be refreshed");
                    return Err(SignInTokenError::ExpiredToken);
                }

                // Refresh the token if it is going to expire soon
                if expires_soon && refreshable {
                    tracing::debug!("attempting to refresh Sign-In token");
                    if let Some(refreshed_token) =
                        Self::refresh_cached_token(&inner, &token, &inner.session_arn, now).await?
                    {
                        token = refreshed_token;
                    }
                    *inner.last_refresh_attempt.lock().unwrap() = Some(now);
                }

                let expires_at = token.expires_at();
                Ok((token, expires_at))
            })
            .await?;

        Ok(token)
    }

    async fn refresh_cached_token(
        inner: &Inner,
        cached_token: &SignInToken,
        identifier: &str,
        now: SystemTime,
    ) -> Result<Option<SignInToken>, SignInTokenError> {
        todo!("invoke sign-in service and attempt refresh")
    }
}

impl ProvideCredentials for SignInCredentialProvider {
    fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
    where
        Self: 'a,
    {
        todo!()
    }
}

/// Builder for [`SignInCredentialProvider`]
#[derive(Debug)]
pub struct Builder {
    session_arn: String,
    provider_config: Option<ProviderConfig>,
}

impl Builder {
    /// Override the configuration used for this provider
    pub fn configure(mut self, provider_config: &ProviderConfig) -> Self {
        self.provider_config = Some(provider_config.clone());
        self
    }

    /// Construct a SignInCredentialsProvider from the builder
    pub fn build(self) -> SignInCredentialProvider {
        let provider_config = self.provider_config.unwrap_or_default();
        let fs = provider_config.fs();
        let env = provider_config.env();
        let inner = Arc::new(Inner {
            fs,
            env,
            session_arn: self.session_arn,
            sdk_config: provider_config.client_config(),
            time_source: provider_config.time_source(),
            last_refresh_attempt: Mutex::new(None),
        });

        SignInCredentialProvider {
            inner,
            token_cache: ExpiringCache::new(REFRESH_BUFFER_TIME),
        }
    }
}
