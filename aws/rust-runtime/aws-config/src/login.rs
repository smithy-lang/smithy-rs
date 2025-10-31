/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Credentials from an AWS Console session vended by AWS Sign-In.

mod cache;
mod dpop;
mod token;

use crate::login::cache::{load_cached_token, save_cached_token, LoginTokenError};
use crate::login::token::LoginToken;
use crate::provider_config::ProviderConfig;
use aws_credential_types::credential_feature::AwsCredentialFeature;
use aws_credential_types::provider;
use aws_credential_types::provider::error::CredentialsError;
use aws_credential_types::provider::future;
use aws_credential_types::provider::ProvideCredentials;
use aws_sdk_signin::config::Builder as SignInClientConfigBuilder;
use aws_sdk_signin::types::CreateOAuth2TokenRequestBody;
use aws_sdk_signin::Client as SignInClient;
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
pub(super) const PROVIDER_NAME: &str = "Login";

/// AWS credentials provider vended by AWS Sign-In. This provider allows users to acquire AWS
/// credentials that correspond to an AWS Console session.
#[derive(Debug)]
pub struct LoginCredentialsProvider {
    inner: Arc<Inner>,
    token_cache: ExpiringCache<LoginToken, LoginTokenError>,
}

#[derive(Debug)]
struct Inner {
    fs: Fs,
    env: Env,
    session_arn: String,
    enabled_from_profile: bool,
    sdk_config: SdkConfig,
    time_source: SharedTimeSource,
    last_refresh_attempt: Mutex<Option<SystemTime>>,
}

impl LoginCredentialsProvider {
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
            enabled_from_profile: false,
        }
    }

    async fn resolve_token(&self) -> Result<LoginToken, LoginTokenError> {
        let token_cache = self.token_cache.clone();
        if let Some(token) = token_cache
            .yield_or_clear_if_expired(self.inner.time_source.now())
            .await
        {
            tracing::debug!("using cached Login token");
            return Ok(token);
        }

        let inner = self.inner.clone();
        let token = token_cache
            .get_or_load(|| async move {
                tracing::debug!("expiring cache asked for an updated Login token");
                let mut token =
                    load_cached_token(&inner.env, &inner.fs, &inner.session_arn).await?;

                tracing::debug!("loaded cached Login token");

                let now = inner.time_source.now();
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
                    "cached Login token refresh decision"
                );

                // Fail fast if the token has expired and we can't refresh it
                if expired && !refreshable {
                    tracing::debug!("cached Login token is expired and cannot be refreshed");
                    return Err(LoginTokenError::ExpiredToken);
                }

                // Refresh the token if it is going to expire soon
                if expires_soon && refreshable {
                    tracing::debug!("attempting to refresh Login token");
                    let refreshed_token = Self::refresh_cached_token(&inner, &token, now).await?;
                    token = refreshed_token;
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
        cached_token: &LoginToken,
        now: SystemTime,
    ) -> Result<LoginToken, LoginTokenError> {
        let dpop_auth_scheme = dpop::DPoPAuthScheme::new(&cached_token.dpop_key)?;
        let client_config = SignInClientConfigBuilder::from(&inner.sdk_config)
            .auth_scheme_resolver(dpop::DPoPAuthSchemeOptionResolver)
            .push_auth_scheme(dpop_auth_scheme)
            .build();

        let client = SignInClient::from_conf(client_config);

        let resp = client
            .create_o_auth2_token()
            .token_input(
                CreateOAuth2TokenRequestBody::builder()
                    .client_id(&cached_token.client_id)
                    .grant_type("refresh_token")
                    .refresh_token(cached_token.refresh_token.as_str())
                    .build()
                    .expect("valid CreateOAuth2TokenRequestBody"),
            )
            .send()
            .await
            .map_err(|e| {
                LoginTokenError::other(
                    "CreateOAuth2Token - failed to refresh token",
                    Some(e.into()),
                )
            })?;

        let token_output = resp.token_output.expect("valid token response");
        let new_token = LoginToken::from_refresh(cached_token, token_output, now);

        match save_cached_token(&inner.env, &inner.fs, &inner.session_arn, &new_token).await {
            Ok(_) => {}
            Err(e) => tracing::warn!("failed to save refreshed Login token: {e}"),
        }
        Ok(new_token)
    }

    async fn credentials(&self) -> provider::Result {
        let token = self
            .resolve_token()
            .await
            // TODO(sign-in): better mapping to CredentialsError
            .map_err(CredentialsError::provider_error)?;

        let feat = match self.inner.enabled_from_profile {
            true => AwsCredentialFeature::CredentialsProfileLogin,
            false => AwsCredentialFeature::CredentialsProfile,
        };

        let mut creds = token.access_token;
        creds
            .get_property_mut_or_default::<Vec<AwsCredentialFeature>>()
            .push(feat);
        Ok(creds)
    }
}

impl ProvideCredentials for LoginCredentialsProvider {
    fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
    where
        Self: 'a,
    {
        future::ProvideCredentials::new(self.credentials())
    }
}

/// Builder for [`LoginCredentialsProvider`]
#[derive(Debug)]
pub struct Builder {
    session_arn: String,
    provider_config: Option<ProviderConfig>,
    enabled_from_profile: bool,
}

impl Builder {
    /// Override the configuration used for this provider
    pub fn configure(mut self, provider_config: &ProviderConfig) -> Self {
        self.provider_config = Some(provider_config.clone());
        self
    }

    /// Set whether this provider was enabled via a profile.
    /// Defaults to `false` (configured explicitly in user code).
    pub(crate) fn enabled_from_profile(mut self, enabled: bool) -> Self {
        self.enabled_from_profile = enabled;
        self
    }

    /// Construct a [`LoginCredentialsProvider`] from the builder
    pub fn build(self) -> LoginCredentialsProvider {
        let provider_config = self.provider_config.unwrap_or_default();
        let fs = provider_config.fs();
        let env = provider_config.env();
        let inner = Arc::new(Inner {
            fs,
            env,
            session_arn: self.session_arn,
            enabled_from_profile: self.enabled_from_profile,
            sdk_config: provider_config.client_config(),
            time_source: provider_config.time_source(),
            last_refresh_attempt: Mutex::new(None),
        });

        LoginCredentialsProvider {
            inner,
            token_cache: ExpiringCache::new(REFRESH_BUFFER_TIME),
        }
    }
}
