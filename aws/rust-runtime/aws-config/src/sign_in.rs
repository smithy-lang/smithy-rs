/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Credentials from an AWS Console session vended by AWS Sign-In.

mod cache;
mod token;

use crate::provider_config::ProviderConfig;
use crate::sign_in::cache::{load_cached_token, save_cached_token, SignInTokenError};
use crate::sign_in::token::SignInToken;
use aws_credential_types::provider;
use aws_credential_types::provider::error::CredentialsError;
use aws_credential_types::provider::future;
use aws_credential_types::provider::ProvideCredentials;
use aws_sdk_signin::types::CreateOAuth2TokenRequestBody;
use aws_sdk_signin::Client as SignInClient;
use aws_smithy_async::time::SharedTimeSource;
use aws_smithy_json::serialize::JsonObjectWriter;
use aws_smithy_runtime::expiring_cache::ExpiringCache;
use aws_smithy_types::Number;
use aws_types::os_shim_internal::{Env, Fs};
use aws_types::SdkConfig;
use p256::ecdsa::{signature::Signer, Signature, SigningKey};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::SecretKey;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::SystemTime;
// TODO(sign-in): fill in additional details on this provider, examples, and links to documentation

const REFRESH_BUFFER_TIME: Duration = Duration::from_secs(5 * 60 /* 5 minutes */);
const MIN_TIME_BETWEEN_REFRESH: Duration = Duration::from_secs(30);
pub(super) const PROVIDER_NAME: &str = "Sign-In";

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

    async fn resolve_token(&self) -> Result<SignInToken, SignInTokenError> {
        let token_cache = self.token_cache.clone();
        if let Some(token) = token_cache
            .yield_or_clear_if_expired(self.inner.time_source.now())
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
        cached_token: &SignInToken,
        now: SystemTime,
    ) -> Result<SignInToken, SignInTokenError> {
        let client = SignInClient::new(&inner.sdk_config);
        // TODO(sign-in): get actual endpoint
        let endpoint = "https://signin.aws.amazon.com/v1/token";
        let dpop = Self::calculate_dpop(cached_token.dpop_key.as_bytes(), endpoint, now)?;
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
            .dpop_proof(dpop)
            .send()
            .await
            .map_err(|e| {
                SignInTokenError::other(
                    "CreateOAuth2Token - failed to refresh token",
                    Some(e.into()),
                )
            })?;

        let token_output = resp.token_output.expect("valid token response");
        let new_token = SignInToken::from_refresh(cached_token, token_output, now);

        match save_cached_token(&inner.env, &inner.fs, &inner.session_arn, &new_token).await {
            Ok(_) => {}
            Err(e) => tracing::warn!("failed to save refreshed Sign-In token: {e}"),
        }
        Ok(new_token)
    }

    /// Calculate DPoP HTTP header using the private key.
    ///
    /// See [RFC 9449: OAuth 2.0 Demonstrating Proof of Possession (DPoP)](https://datatracker.ietf.org/doc/html/rfc9449)
    fn calculate_dpop(
        private_key_pem: &[u8],
        endpoint: &str,
        now: SystemTime,
    ) -> Result<String, SignInTokenError> {
        let private_key = SecretKey::from_slice(private_key_pem)
            .map_err(|e| SignInTokenError::other("invalid secret key", Some(e.into())))?;
        let public_key = private_key.public_key();
        let point = public_key.to_encoded_point(false);
        let x_bytes = point
            .x()
            .ok_or_else(|| SignInTokenError::other("invalid private key: x coordinate", None))?;
        let y_bytes = point
            .y()
            .ok_or_else(|| SignInTokenError::other("invalid private key: y coordinate", None))?;

        let x_b64 = base64_simd::URL_SAFE.encode_to_string(x_bytes);
        let y_b64 = base64_simd::URL_SAFE.encode_to_string(y_bytes);

        let mut header = String::new();
        let mut writer = JsonObjectWriter::new(&mut header);
        writer.key("typ").string("dpop+jwt");
        writer.key("alg").string("ES256");
        let mut jwk = writer.key("jwk").start_object();
        jwk.key("kty").string("EC");
        jwk.key("x").string(&x_b64);
        jwk.key("y").string(&y_b64);
        jwk.key("crv").string("P-256");
        jwk.finish();
        writer.finish();

        let jti = uuid::Uuid::new_v4().to_string();
        let iat = now
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|e| SignInTokenError::other("system time before UNIX epoch", Some(e.into())))?
            .as_secs();

        let mut payload = String::new();
        let mut writer = JsonObjectWriter::new(&mut payload);
        writer.key("jti").string(&jti);
        writer.key("htm").string("POST");
        writer.key("htu").string(endpoint);
        writer.key("iat").number(Number::PosInt(iat));
        writer.finish();

        let header_b64 = base64_simd::URL_SAFE.encode_to_string(header.as_bytes());
        let payload_b64 = base64_simd::URL_SAFE.encode_to_string(payload.as_bytes());
        let message =
            base64_simd::URL_SAFE.encode_to_string(format!("{}.{}", header_b64, payload_b64));

        // Sign the message
        let signing_key = SigningKey::from(&private_key);
        let signature: Signature = signing_key.sign(message.as_bytes());
        let signature_b64 = base64_simd::URL_SAFE.encode_to_string(signature.to_bytes());

        Ok(format!("{}.{}", message, signature_b64))
    }

    async fn credentials(&self) -> provider::Result {
        let token = self
            .resolve_token()
            .await
            // .map(|mut creds| {
            //     creds
            //         .get_property_mut_or_default::<Vec<AwsCredentialFeature>>()
            //         .push(AwsCredentialFeature::CredentialsSso);
            //     creds
            // })
            // TODO(sign-in): better mapping to CredentialsError
            .map_err(CredentialsError::provider_error)?;

        // TODO(sign-in) add credentials properties
        Ok(token.access_token)
    }
}

impl ProvideCredentials for SignInCredentialProvider {
    fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
    where
        Self: 'a,
    {
        future::ProvideCredentials::new(self.credentials())
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
