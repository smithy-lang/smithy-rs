/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![deny(missing_docs)]

//! AWS Shared Config
//!
//! This module contains an shared configuration representation that is agnostic from a specific service.

use std::fmt::Debug;
use std::sync::Arc;

use aws_credential_types::cache::CredentialsCache;
use aws_credential_types::provider::SharedCredentialsProvider;
use aws_smithy_async::rt::sleep::AsyncSleep;
use aws_smithy_async::{AsyncConfiguration, AsyncConfigurationBuilder};
use aws_smithy_client::http_connector::HttpConnector;
use aws_smithy_types::retry::RetryConfig;
use aws_smithy_types::timeout::{TimeoutConfig, TimeoutConfigBuilder};

use crate::app_name::AppName;
use crate::docs_for;
use crate::region::Region;

#[doc(hidden)]
/// Unified docstrings to keep crates in sync. Not intended for public use
pub mod unified_docs {
    #[macro_export]
    macro_rules! docs_for {
        (use_fips) => {
"When true, send this request to the FIPS-compliant regional endpoint.

If no FIPS-compliant endpoint can be determined, dispatching the request will return an error."
        };
        (use_dual_stack) => {
"When true, send this request to the dual-stack endpoint.

If no dual-stack endpoint is available the request MAY return an error.

**Note**: Some services do not offer dual-stack as a configurable parameter (e.g. Code Catalyst). For
these services, this setting has no effect"
        };
    }
}

use aws_smithy_runtime_api::config_bag::{ConfigBag, FrozenConfigBag, Storable};
use aws_smithy_runtime_api::storable;

/// AWS Shared Configuration
#[derive(Debug, Clone)]
pub struct SdkConfig {
    inner: FrozenConfigBag,
}

#[derive(Debug)]
struct UseFips(bool);
storable!(UseFips, mode: replace);

#[derive(Debug)]
struct UseDualStack(bool);
storable!(UseDualStack, mode: replace);

#[derive(Debug)]
struct EndpointUrl(String);
storable!(EndpointUrl, mode: replace);

#[derive(Debug)]
struct RetryConfigStorable(RetryConfig);
storable!(RetryConfigStorable, mode: replace);

#[derive(Debug, Default)]
struct TimeoutConfigStorable(TimeoutConfigBuilder);
impl Storable for TimeoutConfigStorable {
    type ContainerType<T: Send + Sync + Debug> = TimeoutConfigStorable;

    fn merge<'a>(
        accum: Self::ContainerType<&'a Self>,
        item: &'a Self,
    ) -> Self::ContainerType<&'a Self> {
        TimeoutConfigStorable(accum.0.take_unset_from(item.0.clone()))
    }
}

/// Builder for AWS Shared Configuration
///
/// _Important:_ Using the `aws-config` crate to configure the SDK is preferred to invoking this
/// builder directly. Using this builder directly won't pull in any AWS recommended default
/// configuration values.
#[derive(Debug, Default)]
pub struct Builder {
    bag: ConfigBag,
}

impl Builder {
    /// Set the region for the builder
    ///
    /// # Examples
    /// ```rust
    /// use aws_types::SdkConfig;
    /// use aws_types::region::Region;
    /// let config = SdkConfig::builder().region(Region::new("us-east-1")).build();
    /// ```
    pub fn region(mut self, region: impl Into<Option<Region>>) -> Self {
        self.set_region(region);
        self
    }

    /// Set the region for the builder
    ///
    /// # Examples
    /// ```rust
    /// fn region_override() -> Option<Region> {
    ///     // ...
    ///     # None
    /// }
    /// use aws_types::SdkConfig;
    /// use aws_types::region::Region;
    /// let mut builder = SdkConfig::builder();
    /// if let Some(region) = region_override() {
    ///     builder.set_region(region);
    /// }
    /// let config = builder.build();
    /// ```
    pub fn set_region(&mut self, region: impl Into<Option<Region>>) -> &mut Self {
        self.bag.store_or_unset(region.into());
        self
    }

    /// Set the endpoint url to use when making requests.
    /// # Examples
    /// ```
    /// use aws_types::SdkConfig;
    /// let config = SdkConfig::builder().endpoint_url("http://localhost:8080").build();
    /// ```
    pub fn endpoint_url(mut self, endpoint_url: impl Into<String>) -> Self {
        self.set_endpoint_url(Some(endpoint_url.into()));
        self
    }

    /// Set the endpoint url to use when making requests.
    pub fn set_endpoint_url(&mut self, endpoint_url: Option<String>) -> &mut Self {
        self.bag.store_or_unset(endpoint_url.map(EndpointUrl));
        self
    }

    /// Set the retry_config for the builder
    ///
    /// _Note:_ Retries require a sleep implementation in order to work. When enabling retry, make
    /// sure to set one with [Self::sleep_impl] or [Self::set_sleep_impl].
    ///
    /// # Examples
    /// ```rust
    /// use aws_types::SdkConfig;
    /// use aws_smithy_types::retry::RetryConfig;
    ///
    /// let retry_config = RetryConfig::standard().with_max_attempts(5);
    /// let config = SdkConfig::builder().retry_config(retry_config).build();
    /// ```
    pub fn retry_config(mut self, retry_config: RetryConfig) -> Self {
        self.set_retry_config(Some(retry_config));
        self
    }

    /// Set the retry_config for the builder
    ///
    /// _Note:_ Retries require a sleep implementation in order to work. When enabling retry, make
    /// sure to set one with [Self::sleep_impl] or [Self::set_sleep_impl].
    ///
    /// # Examples
    /// ```rust
    /// use aws_types::sdk_config::{SdkConfig, Builder};
    /// use aws_smithy_types::retry::RetryConfig;
    ///
    /// fn disable_retries(builder: &mut Builder) {
    ///     let retry_config = RetryConfig::standard().with_max_attempts(1);
    ///     builder.set_retry_config(Some(retry_config));
    /// }
    ///
    /// let mut builder = SdkConfig::builder();
    /// disable_retries(&mut builder);
    /// ```
    pub fn set_retry_config(&mut self, retry_config: Option<RetryConfig>) -> &mut Self {
        self.bag
            .store_or_unset(retry_config.map(RetryConfigStorable));
        self
    }

    /// Set the [`TimeoutConfig`] for the builder
    ///
    /// _Note:_ Timeouts require a sleep implementation in order to work.
    /// When enabling timeouts, be sure to set one with [Self::sleep_impl] or
    /// [Self::set_sleep_impl].
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use std::time::Duration;
    /// use aws_types::SdkConfig;
    /// use aws_smithy_types::timeout::TimeoutConfig;
    ///
    /// let timeout_config = TimeoutConfig::builder()
    ///     .operation_attempt_timeout(Duration::from_secs(2))
    ///     .operation_timeout(Duration::from_secs(5))
    ///     .build();
    /// let config = SdkConfig::builder()
    ///     .timeout_config(timeout_config)
    ///     .build();
    /// ```
    pub fn timeout_config(mut self, timeout_config: TimeoutConfig) -> Self {
        self.set_timeout_config(Some(timeout_config));
        self
    }

    /// Set the [`TimeoutConfig`] for the builder
    ///
    /// _Note:_ Timeouts require a sleep implementation in order to work.
    /// When enabling timeouts, be sure to set one with [Self::sleep_impl] or
    /// [Self::set_sleep_impl].
    ///
    /// # Examples
    /// ```rust
    /// # use std::time::Duration;
    /// use aws_types::sdk_config::{SdkConfig, Builder};
    /// use aws_smithy_types::timeout::TimeoutConfig;
    ///
    /// fn set_preferred_timeouts(builder: &mut Builder) {
    ///     let timeout_config = TimeoutConfig::builder()
    ///         .operation_attempt_timeout(Duration::from_secs(2))
    ///         .operation_timeout(Duration::from_secs(5))
    ///         .build();
    ///     builder.set_timeout_config(Some(timeout_config));
    /// }
    ///
    /// let mut builder = SdkConfig::builder();
    /// set_preferred_timeouts(&mut builder);
    /// let config = builder.build();
    /// ```
    pub fn set_timeout_config(&mut self, timeout_config: Option<TimeoutConfig>) -> &mut Self {
        self.bag
            .store_or_unset(timeout_config.map(|c| TimeoutConfigStorable(c.to_builder())));
        self
    }

    /// Set the sleep implementation for the builder. The sleep implementation is used to create
    /// timeout futures.
    ///
    /// _Note:_ If you're using the Tokio runtime, a `TokioSleep` implementation is available in
    /// the `aws-smithy-async` crate.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::sync::Arc;
    /// use aws_smithy_async::rt::sleep::{AsyncSleep, Sleep};
    /// use aws_types::SdkConfig;
    ///
    /// ##[derive(Debug)]
    /// pub struct ForeverSleep;
    ///
    /// impl AsyncSleep for ForeverSleep {
    ///     fn sleep(&self, duration: std::time::Duration) -> Sleep {
    ///         Sleep::new(std::future::pending())
    ///     }
    /// }
    ///
    /// let sleep_impl = Arc::new(ForeverSleep);
    /// let config = SdkConfig::builder().sleep_impl(sleep_impl).build();
    /// ```
    pub fn sleep_impl(mut self, sleep_impl: Arc<dyn AsyncSleep>) -> Self {
        self.set_sleep_impl(Some(sleep_impl));
        self
    }

    /// Set the sleep implementation for the builder. The sleep implementation is used to create
    /// timeout futures.
    ///
    /// _Note:_ If you're using the Tokio runtime, a `TokioSleep` implementation is available in
    /// the `aws-smithy-async` crate.
    ///
    /// # Examples
    /// ```rust
    /// # use aws_smithy_async::rt::sleep::{AsyncSleep, Sleep};
    /// # use aws_types::sdk_config::{Builder, SdkConfig};
    /// #[derive(Debug)]
    /// pub struct ForeverSleep;
    ///
    /// impl AsyncSleep for ForeverSleep {
    ///     fn sleep(&self, duration: std::time::Duration) -> Sleep {
    ///         Sleep::new(std::future::pending())
    ///     }
    /// }
    ///
    /// fn set_never_ending_sleep_impl(builder: &mut Builder) {
    ///     let sleep_impl = std::sync::Arc::new(ForeverSleep);
    ///     builder.set_sleep_impl(Some(sleep_impl));
    /// }
    ///
    /// let mut builder = SdkConfig::builder();
    /// set_never_ending_sleep_impl(&mut builder);
    /// let config = builder.build();
    /// ```
    pub fn set_sleep_impl(&mut self, sleep_impl: Option<Arc<dyn AsyncSleep>>) -> &mut Self {
        AsyncConfigurationBuilder::from_bag(&mut self.bag).set_sleep_impl(sleep_impl);
        self
    }

    /// Set the [`CredentialsCache`] for the builder
    ///
    /// # Examples
    /// ```rust
    /// use aws_credential_types::cache::CredentialsCache;
    /// use aws_types::SdkConfig;
    /// let config = SdkConfig::builder()
    ///     .credentials_cache(CredentialsCache::lazy())
    ///     .build();
    /// ```
    pub fn credentials_cache(mut self, cache: CredentialsCache) -> Self {
        self.set_credentials_cache(Some(cache));
        self
    }

    /// Set the [`CredentialsCache`] for the builder
    ///
    /// # Examples
    /// ```rust
    /// use aws_credential_types::cache::CredentialsCache;
    /// use aws_types::SdkConfig;
    /// fn override_credentials_cache() -> bool {
    ///   // ...
    ///   # true
    /// }
    ///
    /// let mut builder = SdkConfig::builder();
    /// if override_credentials_cache() {
    ///     builder.set_credentials_cache(Some(CredentialsCache::lazy()));
    /// }
    /// let config = builder.build();
    /// ```
    pub fn set_credentials_cache(&mut self, cache: Option<CredentialsCache>) -> &mut Self {
        self.bag.store_or_unset(cache);
        self
    }

    /// Set the credentials provider for the builder
    ///
    /// # Examples
    /// ```rust
    /// use aws_credential_types::provider::{ProvideCredentials, SharedCredentialsProvider};
    /// use aws_types::SdkConfig;
    /// fn make_provider() -> impl ProvideCredentials {
    ///   // ...
    ///   # use aws_credential_types::Credentials;
    ///   # Credentials::new("test", "test", None, None, "example")
    /// }
    ///
    /// let config = SdkConfig::builder()
    ///     .credentials_provider(SharedCredentialsProvider::new(make_provider()))
    ///     .build();
    /// ```
    pub fn credentials_provider(mut self, provider: SharedCredentialsProvider) -> Self {
        self.set_credentials_provider(Some(provider));
        self
    }

    /// Set the credentials provider for the builder
    ///
    /// # Examples
    /// ```rust
    /// use aws_credential_types::provider::{ProvideCredentials, SharedCredentialsProvider};
    /// use aws_types::SdkConfig;
    /// fn make_provider() -> impl ProvideCredentials {
    ///   // ...
    ///   # use aws_credential_types::Credentials;
    ///   # Credentials::new("test", "test", None, None, "example")
    /// }
    ///
    /// fn override_provider() -> bool {
    ///   // ...
    ///   # true
    /// }
    ///
    /// let mut builder = SdkConfig::builder();
    /// if override_provider() {
    ///     builder.set_credentials_provider(Some(SharedCredentialsProvider::new(make_provider())));
    /// }
    /// let config = builder.build();
    /// ```
    pub fn set_credentials_provider(
        &mut self,
        provider: Option<SharedCredentialsProvider>,
    ) -> &mut Self {
        self.bag.store_or_unset(provider);
        self
    }

    /// Sets the name of the app that is using the client.
    ///
    /// This _optional_ name is used to identify the application in the user agent that
    /// gets sent along with requests.
    pub fn app_name(mut self, app_name: AppName) -> Self {
        self.set_app_name(Some(app_name));
        self
    }

    /// Sets the name of the app that is using the client.
    ///
    /// This _optional_ name is used to identify the application in the user agent that
    /// gets sent along with requests.
    pub fn set_app_name(&mut self, app_name: Option<AppName>) -> &mut Self {
        self.bag.store_or_unset(app_name);
        self
    }

    /// Sets the HTTP connector to use when making requests.
    ///
    /// ## Examples
    /// ```no_run
    /// # #[cfg(feature = "examples")]
    /// # fn example() {
    /// use std::time::Duration;
    /// use aws_smithy_client::{Client, hyper_ext};
    /// use aws_smithy_client::erase::DynConnector;
    /// use aws_smithy_client::http_connector::ConnectorSettings;
    /// use aws_types::SdkConfig;
    ///
    /// let https_connector = hyper_rustls::HttpsConnectorBuilder::new()
    ///     .with_webpki_roots()
    ///     .https_only()
    ///     .enable_http1()
    ///     .enable_http2()
    ///     .build();
    /// let smithy_connector = hyper_ext::Adapter::builder()
    ///     // Optionally set things like timeouts as well
    ///     .connector_settings(
    ///         ConnectorSettings::builder()
    ///             .connect_timeout(Duration::from_secs(5))
    ///             .build()
    ///     )
    ///     .build(https_connector);
    /// let sdk_config = SdkConfig::builder()
    ///     .http_connector(smithy_connector)
    ///     .build();
    /// # }
    /// ```
    pub fn http_connector(mut self, http_connector: impl Into<HttpConnector>) -> Self {
        self.set_http_connector(Some(http_connector));
        self
    }

    /// Sets the HTTP connector to use when making requests.
    ///
    /// ## Examples
    /// ```no_run
    /// # #[cfg(feature = "examples")]
    /// # fn example() {
    /// use std::time::Duration;
    /// use aws_smithy_client::hyper_ext;
    /// use aws_smithy_client::http_connector::ConnectorSettings;
    /// use aws_types::sdk_config::{SdkConfig, Builder};
    ///
    /// fn override_http_connector(builder: &mut Builder) {
    ///     let https_connector = hyper_rustls::HttpsConnectorBuilder::new()
    ///         .with_webpki_roots()
    ///         .https_only()
    ///         .enable_http1()
    ///         .enable_http2()
    ///         .build();
    ///     let smithy_connector = hyper_ext::Adapter::builder()
    ///         // Optionally set things like timeouts as well
    ///         .connector_settings(
    ///             ConnectorSettings::builder()
    ///                 .connect_timeout(Duration::from_secs(5))
    ///                 .build()
    ///         )
    ///         .build(https_connector);
    ///     builder.set_http_connector(Some(smithy_connector));
    /// }
    ///
    /// let mut builder = SdkConfig::builder();
    /// override_http_connector(&mut builder);
    /// let config = builder.build();
    /// # }
    /// ```
    pub fn set_http_connector(
        &mut self,
        http_connector: Option<impl Into<HttpConnector>>,
    ) -> &mut Self {
        self.bag.store_or_unset(http_connector.map(|c| c.into()));
        self
    }

    #[doc = docs_for!(use_fips)]
    pub fn use_fips(mut self, use_fips: bool) -> Self {
        self.set_use_fips(Some(use_fips));
        self
    }

    #[doc = docs_for!(use_fips)]
    pub fn set_use_fips(&mut self, use_fips: Option<bool>) -> &mut Self {
        self.bag.store_or_unset(use_fips.map(UseFips));
        self
    }

    #[doc = docs_for!(use_dual_stack)]
    pub fn use_dual_stack(mut self, use_dual_stack: bool) -> Self {
        self.set_use_dual_stack(Some(use_dual_stack));
        self
    }

    #[doc = docs_for!(use_dual_stack)]
    pub fn set_use_dual_stack(&mut self, use_dual_stack: Option<bool>) -> &mut Self {
        self.bag.store_or_unset(use_dual_stack.map(UseDualStack));
        self
    }

    /// Build a [`SdkConfig`](SdkConfig) from this builder
    pub fn build(self) -> SdkConfig {
        SdkConfig {
            inner: self.bag.freeze(),
        }
    }
}

impl SdkConfig {
    /// Override this builder with settings from `other`
    pub fn override_with(&self, other: Builder) -> Self {
        SdkConfig {
            inner: self
                .inner
                .add_bag_layer("SdkConfig override", other.bag)
                .freeze(),
        }
    }
    /// Configured region
    pub fn region(&self) -> Option<&Region> {
        self.inner.load::<Region>()
    }

    /// Configured endpoint URL
    pub fn endpoint_url(&self) -> Option<&str> {
        self.inner.load::<EndpointUrl>().map(|e| e.0.as_ref())
    }

    /// Configured retry config
    pub fn retry_config(&self) -> Option<&RetryConfig> {
        self.inner.load::<RetryConfigStorable>().map(|r| &r.0)
    }

    /// Configured timeout config
    pub fn timeout_config(&self) -> Option<TimeoutConfig> {
        Some(self.inner.load::<TimeoutConfigStorable>().0.build())
    }

    #[doc(hidden)]
    /// Configured sleep implementation
    pub fn sleep_impl(&self) -> Option<Arc<dyn AsyncSleep>> {
        aws_smithy_async::AsyncConfiguration::from_bag(&self.inner).async_sleep()
    }

    /// Configured credentials cache
    pub fn credentials_cache(&self) -> Option<&CredentialsCache> {
        todo!()
    }

    /// Configured credentials provider
    pub fn credentials_provider(&self) -> Option<&SharedCredentialsProvider> {
        todo!()
    }

    /// Configured app name
    pub fn app_name(&self) -> Option<&AppName> {
        todo!()
    }

    /// Configured HTTP Connector
    pub fn http_connector(&self) -> Option<&HttpConnector> {
        todo!()
    }

    /// Use FIPS endpoints
    pub fn use_fips(&self) -> Option<bool> {
        self.inner.load::<UseFips>().map(|u| u.0)
    }

    /// Use dual-stack endpoint
    pub fn use_dual_stack(&self) -> Option<bool> {
        self.inner.load::<UseDualStack>().map(|u| u.0)
    }

    /// Config builder
    ///
    /// _Important:_ Using the `aws-config` crate to configure the SDK is preferred to invoking this
    /// builder directly. Using this builder directly won't pull in any AWS recommended default
    /// configuration values.
    pub fn builder() -> Builder {
        Builder::default()
    }
}

#[cfg(test)]
mod test {
    use crate::region::Region;
    use crate::SdkConfig;
    use aws_smithy_types::timeout::TimeoutConfig;
    use std::time::Duration;

    #[test]
    fn sdk_config_layering_tests() {
        let base_config = SdkConfig::builder()
            .region(Region::from_static("us-east-1"))
            .timeout_config(
                TimeoutConfig::builder()
                    .operation_timeout(Duration::from_secs(5))
                    .build(),
            )
            .build();

        let layered_with_endpoint = base_config.override_with(
            SdkConfig::builder()
                .endpoint_url("http://localhost:8000")
                .timeout_config(
                    TimeoutConfig::builder()
                        .connect_timeout(Duration::from_secs(3))
                        .build(),
                ),
        );

        assert_eq!(
            base_config.region(),
            Some(&Region::from_static("us-east-1"))
        );

        assert_eq!(base_config.endpoint_url(), None);
        assert_eq!(
            layered_with_endpoint.endpoint_url(),
            Some("http://localhost:8000")
        );

        assert_eq!(
            layered_with_endpoint.timeout_config(),
            Some(
                TimeoutConfig::builder()
                    .operation_timeout(Duration::from_secs(5))
                    .connect_timeout(Duration::from_secs(3))
                    .build()
            )
        )
    }
}
