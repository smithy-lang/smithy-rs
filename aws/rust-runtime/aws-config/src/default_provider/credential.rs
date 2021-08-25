/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::borrow::Cow;

use crate::meta::credential::lazy_caching;
use crate::meta::credential::lazy_caching::LazyCachingCredentialsProvider;
use aws_types::credential::provide_credentials::future;
use aws_types::credential::ProvideCredentials;
use aws_types::os_shim_internal::{Env, Fs};
use aws_types::region::Region;
use smithy_async::rt::sleep::AsyncSleep;
use smithy_client::DynConnector;

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
/// let credentials_provider = aws_config::default_provider::credential::DefaultProvider::builder()
///     .region(Region::new("us-west-1"))
///     .build();
/// ```
///
/// Create a default chain with no overrides:
/// ```rust
/// let credentials_provider = aws_config::default_provider::credential::default_provider();
/// ```
pub struct DefaultProvider(LazyCachingCredentialsProvider);

pub async fn default_provider() -> impl ProvideCredentials {
    use crate::meta::region::ProvideRegion;
    let region = super::region::default_provider().region().await;
    Builder::default().region(region).build()
}

impl DefaultProvider {
    pub fn builder() -> Builder {
        Builder::default()
    }
}

impl ProvideCredentials for DefaultProvider {
    fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
    where
        Self: 'a,
    {
        future::ProvideCredentials::new(self.0.provide_credentials())
    }
}

/// Builder for [`DefaultProviderChain`](DefaultProviderChain)
#[derive(Default)]
pub struct Builder {
    profile_file_builder: crate::profile::credential::Builder,
    web_identity_builder: crate::web_identity_token::Builder,
    credential_cache: lazy_caching::builder::Builder,
    env: Option<Env>,
}

impl Builder {
    /// Set the region used when making requests to AWS services (eg. STS) as part of the provider chain
    ///
    /// When unset, the default region resolver chain will be used.
    pub fn region(mut self, region: impl Into<Option<Region>>) -> Self {
        self.set_region(region.into());
        self
    }

    pub fn set_region(&mut self, region: Option<Region>) -> &mut Self {
        self.profile_file_builder.set_region(region.clone());
        self.web_identity_builder.set_region(region);
        self
    }

    /// Override the HTTPS connector used for this provider
    ///
    /// If a connector other than Hyper is used or if the Tokio/Hyper features have been disabled
    /// this method MUST be used to specify a custom connector.
    pub fn connector(mut self, connector: DynConnector) -> Self {
        self.profile_file_builder
            .set_connector(Some(connector.clone()));
        self.web_identity_builder.set_connector(Some(connector));
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
    /// role_arn = arn:aws:iam::123456789:role/RoleA
    /// credential_source = MyCustomProvider
    /// ```
    ///
    /// Typically, these are built-in providers like `Environment`, however, custom sources may
    /// also be used. Using custom sources must be registered:
    /// ```rust
    /// use aws_config::default_provider::credential::DefaultProvider;
    /// use aws_types::credential::{ProvideCredentials, CredentialsError};
    /// use aws_types::Credentials;
    /// use aws_types::credential::provide_credentials::future;
    /// struct MyCustomProvider;
    /// impl ProvideCredentials for MyCustomProvider {
    ///     fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a> where Self: 'a {
    ///         todo!()
    ///     }
    /// }
    /// // assume role can now use `MyCustomProvider`
    /// let provider_chain = DefaultProvider::builder()
    ///     .with_custom_credential_source("MyCustomProvider", MyCustomProvider)
    ///     .build();
    /// ```
    pub fn with_custom_credential_source(
        mut self,
        name: impl Into<Cow<'static, str>>,
        provider: impl ProvideCredentials + 'static,
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
        self.profile_file_builder.set_fs(fs.clone());
        self.web_identity_builder.set_fs(fs);
        self
    }

    #[doc(hidden)]
    /// Override the environment used for this provider
    ///
    /// This method exists primarily for testing credential providers
    pub fn env(mut self, env: Env) -> Self {
        self.env = Some(env.clone());
        self.profile_file_builder.set_env(env.clone());
        self.web_identity_builder.set_env(env);
        self
    }

    pub fn build(self) -> DefaultProvider {
        let profile_provider = self.profile_file_builder.build();
        let env_provider =
            crate::environment::credential::EnvironmentVariableCredentialsProvider::new_with_env(
                self.env.unwrap_or_default(),
            );
        let web_identity_token_provider = self.web_identity_builder.build();
        let provider_chain = crate::meta::credential::chain::CredentialsProviderChain::first_try(
            "Environment",
            env_provider,
        )
        .or_else("Profile", profile_provider)
        .or_else("WebIdentityToken", web_identity_token_provider);
        let cached_provider = self.credential_cache.load(provider_chain);
        DefaultProvider(cached_provider.build())
    }
}

#[cfg(test)]
mod test {

    macro_rules! make_test {
        ($name: ident) => {
            #[traced_test]
            #[tokio::test]
            async fn $name() {
                crate::test_case::TestEnvironment::from_dir(concat!(
                    "./test-data/default-provider-chain/",
                    stringify!($name)
                ))
                .unwrap()
                .execute(|fs, env, conn| {
                    crate::default_provider::credential::Builder::default()
                        .env(env)
                        .fs(fs)
                        .region(Region::from_static("us-east-1"))
                        .connector(conn)
                        .build()
                })
                .await
            }
        };
    }

    use aws_sdk_sts::Region;

    use tracing_test::traced_test;

    make_test!(prefer_environment);
    make_test!(profile_static_keys);
    make_test!(web_identity_token_env);
    make_test!(web_identity_source_profile_no_env);
    make_test!(web_identity_token_invalid_jwt);
    make_test!(web_identity_token_source_profile);
    make_test!(web_identity_token_profile);
    make_test!(profile_overrides_web_identity);

    /// Helper that uses `execute_and_update` instead of execute
    ///
    /// If you run this, it will add another HTTP traffic log which re-records the request
    /// data
    #[tokio::test]
    #[ignore]
    async fn update_test() {
        crate::test_case::TestEnvironment::from_dir(concat!(
            "./test-data/default-provider-chain/web_identity_token_source_profile",
        ))
        .unwrap()
        .execute_and_update(|fs, env, conn| {
            crate::default_provider::credential::Builder::default()
                .env(env)
                .fs(fs)
                .region(Region::from_static("us-east-1"))
                .connector(conn)
                .build()
        })
        .await
    }
}
