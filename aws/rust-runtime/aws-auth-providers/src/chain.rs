/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::borrow::Cow;

use aws_auth::provider::{AsyncProvideCredentials, BoxFuture, CredentialsError, CredentialsResult};
use tracing::Instrument;

/// Credentials provider that checks a series of inner providers
///
/// Each provider will be checked in turn. The first provider that returns a successful credential
/// will be used.
///
/// ## Example
/// ```rust
/// use aws_auth_providers::chain::ChainProvider;
/// use aws_auth::provider::env::EnvironmentVariableCredentialsProvider;
/// use aws_auth::Credentials;
/// let provider = ChainProvider::first_try("Environment", EnvironmentVariableCredentialsProvider::new())
///     .or_else("Static", Credentials::from_keys("someacceskeyid", "somesecret", None));
/// ```
pub struct ChainProvider {
    providers: Vec<(Cow<'static, str>, Box<dyn AsyncProvideCredentials>)>,
}

impl ChainProvider {
    pub fn first_try(
        name: impl Into<Cow<'static, str>>,
        provider: impl AsyncProvideCredentials + 'static,
    ) -> Self {
        ChainProvider {
            providers: vec![(name.into(), Box::new(provider))],
        }
    }

    pub fn or_else(
        mut self,
        name: impl Into<Cow<'static, str>>,
        provider: impl AsyncProvideCredentials + 'static,
    ) -> Self {
        self.providers.push((name.into(), Box::new(provider)));
        self
    }

    async fn credentials(&self) -> CredentialsResult {
        let mut last_error = CredentialsError::Unhandled("no providers".into());
        for (name, provider) in &self.providers {
            let span = tracing::info_span!("load_credentials", provider = %name);
            match provider.provide_credentials().instrument(span).await {
                Ok(credentials) => {
                    tracing::info!(provider = %name, "loaded credentials");
                    return Ok(credentials);
                }
                Err(e) => {
                    tracing::info!(provider = %name, error = %e, "provider in chain did not provide credentials");
                    last_error = e
                }
            }
        }
        return Err(last_error);
    }
}

impl AsyncProvideCredentials for ChainProvider {
    fn provide_credentials<'a>(&'a self) -> BoxFuture<'a, CredentialsResult>
    where
        Self: 'a,
    {
        Box::pin(self.credentials())
    }
}
