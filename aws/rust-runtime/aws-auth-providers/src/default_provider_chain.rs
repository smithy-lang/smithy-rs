/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::borrow::Cow;

use aws_auth::provider::env::EnvironmentVariableCredentialsProvider;
use aws_auth::provider::lazy_caching::LazyCachingCredentialsProvider;
use aws_auth::provider::BoxFuture;
use aws_auth::provider::{AsyncProvideCredentials, CredentialsResult};
use aws_hyper::DynConnector;
use aws_types::os_shim_internal::{Env, Fs};
use aws_types::region::ProvideRegion;
use smithy_async::rt::sleep::AsyncSleep;

/// Default AWS Credential Provider Chain
///
/// Resolution order:
/// 1. Environment variables: [`EnvironmentVariableCredentialsProvider`](aws_auth::provider::env::EnvironmentVariableCredentialsProvider)
/// 2. Shared config (`~/.aws/config`, `~/.aws/credentials`): [`SharedConfigCredentialsProvider`](crate::profile::ProfileFileCredentialProvider)
///
/// The outer provider is wrapped in a refreshing cache.
///
/// More providers are a work in progress.
///
/// ## Example:
/// Create a default chain with a custom region:
/// ```rust
/// use aws_types::region::Region;
/// let credentials_provider = aws_auth_providers::DefaultProviderChain::builder()
///     .region(&Region::new("us-west-1"))
///     .build();
/// ```
///
/// Create a default chain with no overrides:
/// ```rust
/// let credentials_provider = aws_auth_providers::default_provider();
/// ```
pub struct DefaultProviderChain(LazyCachingCredentialsProvider);

impl DefaultProviderChain {
    pub fn builder() -> Builder {
        Builder::default()
    }
}

impl AsyncProvideCredentials for DefaultProviderChain {
    fn provide_credentials<'a>(&'a self) -> BoxFuture<'a, CredentialsResult>
    where
        Self: 'a,
    {
        self.0.provide_credentials()
    }
}

/// Builder for [`DefaultProviderChain`](DefaultProviderChain)
#[derive(Default)]
pub struct Builder {
    profile_file_builder: crate::profile::Builder,
    credential_cache: aws_auth::provider::lazy_caching::builder::Builder,
    env: Option<Env>,
}

impl Builder {
    /// Set the region used when making requests to AWS services (eg. STS) as part of the provider chain
    ///
    /// When unset, the default region resolver chain will be used.
    pub fn region(mut self, region: &dyn ProvideRegion) -> Self {
        self.profile_file_builder.set_region(region.region());
        self
    }

    /// Override the HTTPS connector used for this provider
    ///
    /// If a connector other than Hyper is used or if the Tokio/Hyper features have been disabled
    /// this method MUST be used to specify a custom connector.
    pub fn connector(mut self, connector: DynConnector) -> Self {
        self.profile_file_builder.set_connector(Some(connector));
        self
    }

    /// Override the sleep implementation used for this provider
    ///
    /// By default, Tokio will be used to support async sleep during credentials for timeouts
    /// and reloading credentials. If the tokio default feature has been disabled, a custom
    /// sleep implementation must be provided.
    pub fn sleep(mut self, sleep: impl AsyncSleep + 'static) -> Self {
        self.credential_cache = self.credential_cache.sleep(sleep);
        self
    }

    /// Add an additional credential source for the ProfileProvider
    ///
    /// Assume role profiles may specify named credential sources:
    /// ```ini
    /// [default]
    /// ```ini
    /// [default]
    /// role_arn = arn:aws:iam::123456789:role/RoleA
    /// credential_source = MyCustomProvider
    /// ```
    ///
    /// Typically, these are built-in providers like `Environment`, however, custom sources may
    /// also be used. Using custom sources must be registered:
    /// ```rust
    /// use aws_auth::provider::{ProvideCredentials, CredentialsError};
    /// use aws_auth::Credentials;
    /// use aws_auth_providers::DefaultProviderChain;
    /// struct MyCustomProvider;
    /// // there is a blanket implementation for `AsyncProvideCredentials` on ProvideCredentials
    /// impl ProvideCredentials for MyCustomProvider {
    ///   fn provide_credentials(&self) -> Result<Credentials, CredentialsError> {
    ///     todo!()
    ///   }
    /// }
    /// // assume role can now use `MyCustomProvider` when maed
    /// let provider_chain = DefaultProviderChain::builder()
    ///     .with_custom_credential_source("MyCustomProvider", MyCustomProvider)
    ///     .build();
    pub fn with_custom_credential_source(
        mut self,
        name: impl Into<Cow<'static, str>>,
        provider: impl AsyncProvideCredentials + 'static,
    ) -> Self {
        self.profile_file_builder = self
            .profile_file_builder
            .with_custom_provider(name, provider);
        self
    }

    #[doc(hidden)]
    /// Override the filesystem used for this provider
    ///
    /// This method exists primarily for testing credential providers
    pub fn fs(mut self, fs: Fs) -> Self {
        self.profile_file_builder.set_fs(Some(fs));
        self
    }

    #[doc(hidden)]
    /// Override the environment used for this provider
    ///
    /// This method exists primarily for testing credential providers
    pub fn env(mut self, env: Env) -> Self {
        self.env = Some(env.clone());
        self.profile_file_builder.set_env(Some(env));
        self
    }

    pub fn build(self) -> DefaultProviderChain {
        let profile_provider = self.profile_file_builder.build();
        let env_provider =
            EnvironmentVariableCredentialsProvider::new_with_env(self.env.unwrap_or_default());
        let provider_chain = crate::chain::ChainProvider::first_try("Environment", env_provider)
            .or_else("Profile", profile_provider);
        let cached_provider = self.credential_cache.load(provider_chain);
        DefaultProviderChain(cached_provider.build())
    }
}

#[cfg(test)]
mod test {
    use crate::DefaultProviderChain;
    use aws_auth::provider::AsyncProvideCredentials;
    use aws_hyper::DynConnector;
    use aws_types::os_shim_internal::{Env, Fs};
    use smithy_client::dvr::ReplayingConnection;
    use tracing_test::traced_test;

    #[tokio::test]
    async fn prefer_environment() {
        let env = Env::from_slice(&[
            ("AWS_ACCESS_KEY_ID", "correct_key"),
            ("AWS_SECRET_ACCESS_KEY", "correct_secret"),
            ("HOME", "/Users/me"),
        ]);

        let fs = Fs::from_test_dir("test-data/aws-config/e2e-assume-role", "/Users/me");
        // empty connection will error if it is used
        let connection = ReplayingConnection::new(vec![]);
        let provider = DefaultProviderChain::builder()
            .fs(fs)
            .env(env)
            .connector(DynConnector::new(connection))
            .build();
        // empty connection will error if it is used
        let creds = provider.provide_credentials().await.expect("valid creds");
        assert_eq!(creds.access_key_id(), "correct_key");
        assert_eq!(creds.secret_access_key(), "correct_secret")
    }

    #[traced_test]
    #[tokio::test]
    async fn fallback_to_profile() {
        let env = Env::from_slice(&[
            // access keys not in environment
            ("HOME", "/Users/me"),
        ]);

        let fs = Fs::from_test_dir("./test-data/static-keys/aws-config", "/Users/me/.aws");
        // empty connection will error if it is used
        let connection = ReplayingConnection::new(vec![]);
        let provider = DefaultProviderChain::builder()
            .fs(fs)
            .env(env)
            .connector(DynConnector::new(connection))
            .build();
        let creds = provider.provide_credentials().await.expect("valid creds");
        assert_eq!(creds.access_key_id(), "correct_key");
        assert_eq!(creds.secret_access_key(), "correct_secret")
    }
}
