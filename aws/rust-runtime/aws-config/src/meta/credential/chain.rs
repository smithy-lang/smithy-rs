/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::borrow::Cow;

use aws_types::credential;
use aws_types::credential::provide_credentials::future;
use aws_types::credential::{CredentialsError, ProvideCredentials};
use tracing::Instrument;

/// Credentials provider that checks a series of inner providers
///
/// Each provider will be checked in turn. The first provider that returns a successful credential
/// will be used.
///
/// ## Example
/// ```rust
/// use aws_config::meta::credential::chain::ProviderChain;
/// use aws_types::Credentials;
/// use aws_config::environment;
/// let provider = ProviderChain::first_try("Environment", environment::credential::Provider::new())
///     .or_else("Static", Credentials::from_keys("someacceskeyid", "somesecret", None));
/// ```
pub struct ProviderChain {
    providers: Vec<(Cow<'static, str>, Box<dyn ProvideCredentials>)>,
}

impl ProviderChain {
    pub fn first_try(
        name: impl Into<Cow<'static, str>>,
        provider: impl ProvideCredentials + 'static,
    ) -> Self {
        ProviderChain {
            providers: vec![(name.into(), Box::new(provider))],
        }
    }

    pub fn or_else(
        mut self,
        name: impl Into<Cow<'static, str>>,
        provider: impl ProvideCredentials + 'static,
    ) -> Self {
        self.providers.push((name.into(), Box::new(provider)));
        self
    }

    async fn credentials(&self) -> credential::Result {
        for (name, provider) in &self.providers {
            let span = tracing::info_span!("load_credentials", provider = %name);
            match provider.provide_credentials().instrument(span).await {
                Ok(credentials) => {
                    tracing::info!(provider = %name, "loaded credentials");
                    return Ok(credentials);
                }
                Err(CredentialsError::CredentialsNotLoaded) => {
                    tracing::info!(provider = %name, "provider in chain did not provide credentials");
                }
                Err(e) => {
                    tracing::warn!(provider = %name, error = %e, "provider failed to provide credentials");
                    return Err(e);
                }
            }
        }
        return Err(CredentialsError::CredentialsNotLoaded);
    }
}

impl ProvideCredentials for ProviderChain {
    fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials
    where
        Self: 'a,
    {
        future::ProvideCredentials::new(self.credentials())
    }
}
