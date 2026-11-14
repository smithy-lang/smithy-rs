/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Configuration for a dynamic client.

pub use aws_smithy_dynamic_client_api::config_key::{
    Applies, ConfigKey, EndpointBuiltIn, InvalidEndpointBuiltIn,
};

pub use aws_smithy_runtime::client::identity::IdentityCache;
pub use aws_smithy_runtime::client::retries::RetryPartition;
pub use aws_smithy_runtime_api::client::behavior_version::BehaviorVersion;
pub use aws_smithy_runtime_api::client::stalled_stream_protection::StalledStreamProtectionConfig;
pub use aws_smithy_types::retry::RetryConfig;
pub use aws_smithy_types::timeout::TimeoutConfig;
#[cfg(feature = "aws")]
pub use aws_types::region::Region;

use aws_smithy_async::rt::sleep::{AsyncSleep, SharedAsyncSleep};
use aws_smithy_async::time::{SharedTimeSource, TimeSource};
use aws_smithy_runtime_api::client::http::{HttpClient, SharedHttpClient};
use aws_smithy_runtime_api::client::identity::{ResolveCachedIdentity, SharedIdentityCache};
use aws_smithy_runtime_api::client::interceptors::{Intercept, SharedInterceptor};
use aws_smithy_runtime_api::client::retries::classifiers::{ClassifyRetry, SharedRetryClassifier};
use aws_smithy_runtime_api::client::runtime_plugin::{RuntimePlugin, SharedRuntimePlugin};
use aws_smithy_schema::protocol::{ClientProtocol, SharedClientProtocol};
use aws_smithy_types::config_bag::Layer;
use std::any::{type_name, TypeId};
use std::fmt;
use std::sync::Arc;
#[cfg(feature = "aws")]
use {
    aws_credential_types::provider::{ProvideCredentials, SharedCredentialsProvider},
    aws_runtime::invocation_id::{InvocationIdGenerator, SharedInvocationIdGenerator},
    aws_types::SdkConfig,
};

/// Configuration for a [`DynamicClient`](crate::DynamicClient).
///
/// Pass it to the client builder with [`config`](crate::client::DynamicClientBuilder::config).
///
/// ```
/// use aws_smithy_dynamic_client::config::{BehaviorVersion, Config};
///
/// let config = Config::builder()
///     .behavior_version(BehaviorVersion::latest())
///     .build();
/// ```
#[derive(Clone, Debug)]
pub struct Config {
    builder: Builder,
}

impl Config {
    /// Returns a builder for a config.
    pub fn builder() -> Builder {
        Builder::default()
    }
}

/// A builder for a [`Config`].
#[derive(Clone, Debug, Default)]
pub struct Builder {
    pub(crate) behavior_version: Option<BehaviorVersion>,
    pub(crate) protocol: Option<SharedClientProtocol>,
    pub(crate) http_client: Option<SharedHttpClient>,
    pub(crate) sleep_impl: Option<SharedAsyncSleep>,
    pub(crate) time_source: Option<SharedTimeSource>,
    pub(crate) retry_config: Option<RetryConfig>,
    pub(crate) timeout_config: Option<TimeoutConfig>,
    pub(crate) retry_partition: Option<RetryPartition>,
    pub(crate) identity_cache: Option<SharedIdentityCache>,
    pub(crate) stalled_stream_protection: Option<StalledStreamProtectionConfig>,
    pub(crate) retry_classifiers: Vec<SharedRetryClassifier>,
    pub(crate) interceptors: Vec<SharedInterceptor>,
    pub(crate) runtime_plugins: Vec<SharedRuntimePlugin>,
    // The values set by `put`, in the order they were set. A key set twice is listed twice.
    pub(crate) puts: Vec<Put>,
    #[cfg(feature = "aws")]
    pub(crate) region: Option<Region>,
    #[cfg(feature = "aws")]
    pub(crate) credentials_provider: Option<SharedCredentialsProvider>,
    #[cfg(feature = "aws")]
    pub(crate) invocation_id_generator: Option<SharedInvocationIdGenerator>,
    #[cfg(feature = "aws")]
    pub(crate) disable_clock_skew_correction: Option<bool>,
}

impl Builder {
    /// Sets the behavior major version.
    ///
    /// A behavior version is required. It pins default behavior, so a client keeps its
    /// behavior when the crate is upgraded.
    pub fn behavior_version(mut self, behavior_version: BehaviorVersion) -> Self {
        self.behavior_version = Some(behavior_version);
        self
    }

    /// Sets the protocol to use, in place of the one detected from the service's protocol
    /// traits.
    pub fn protocol(mut self, protocol: impl ClientProtocol + 'static) -> Self {
        self.protocol = Some(SharedClientProtocol::new(protocol));
        self
    }

    /// Sets the HTTP client that sends requests.
    ///
    /// Without one, the default HTTP client is used. The default client needs the
    /// `default-https-client` feature. The feature is on by default.
    pub fn http_client(mut self, http_client: impl HttpClient + 'static) -> Self {
        self.http_client = Some(SharedHttpClient::new(http_client));
        self
    }

    /// Sets the async sleep implementation used for retries and timeouts.
    ///
    /// Without one, the Tokio sleep is used. The Tokio sleep needs the `rt-tokio` feature. The
    /// feature is on by default.
    pub fn sleep_impl(mut self, sleep_impl: impl AsyncSleep + 'static) -> Self {
        self.sleep_impl = Some(SharedAsyncSleep::new(sleep_impl));
        self
    }

    /// Sets the time source that the client reads the current time from.
    ///
    /// Without one, the system clock is used. A target with no system clock, such as
    /// `wasm32-unknown-unknown`, needs a time source.
    pub fn time_source(mut self, time_source: impl TimeSource + 'static) -> Self {
        self.time_source = Some(SharedTimeSource::new(time_source));
        self
    }

    /// Sets the retry config.
    ///
    /// Without one, the default for the behavior version is used.
    pub fn retry_config(mut self, retry_config: RetryConfig) -> Self {
        self.retry_config = Some(retry_config);
        self
    }

    /// Sets the timeout config.
    ///
    /// Without one, the default for the behavior version is used.
    pub fn timeout_config(mut self, timeout_config: TimeoutConfig) -> Self {
        self.timeout_config = Some(timeout_config);
        self
    }

    /// Sets the retry partition. Clients in the same partition share retry state, such as the
    /// retry token bucket.
    ///
    /// Without one, the partition is named after the service shape.
    pub fn retry_partition(mut self, retry_partition: RetryPartition) -> Self {
        self.retry_partition = Some(retry_partition);
        self
    }

    /// Sets the identity cache that holds resolved identities, such as credentials, until they
    /// expire.
    ///
    /// Without one, the default lazy cache is used.
    pub fn identity_cache(mut self, identity_cache: impl ResolveCachedIdentity + 'static) -> Self {
        self.identity_cache = Some(SharedIdentityCache::new(identity_cache));
        self
    }

    /// Sets the stalled stream protection config.
    ///
    /// Without one, the default for the behavior version is used.
    pub fn stalled_stream_protection(
        mut self,
        stalled_stream_protection: StalledStreamProtectionConfig,
    ) -> Self {
        self.stalled_stream_protection = Some(stalled_stream_protection);
        self
    }

    /// Adds a retry classifier. Each call adds one more.
    pub fn retry_classifier(mut self, retry_classifier: impl ClassifyRetry + 'static) -> Self {
        self.retry_classifiers
            .push(SharedRetryClassifier::new(retry_classifier));
        self
    }

    /// Adds an interceptor. Each call adds one more, and interceptors run in the order they are
    /// added.
    pub fn interceptor(mut self, interceptor: impl Intercept + 'static) -> Self {
        self.interceptors.push(SharedInterceptor::new(interceptor));
        self
    }

    /// Adds a runtime plugin. Each call adds one more.
    ///
    /// Runtime plugins are applied after the rest of the config, in the order they are added,
    /// so a plugin can override a value set here.
    pub fn runtime_plugin(mut self, runtime_plugin: impl RuntimePlugin + 'static) -> Self {
        self.runtime_plugins
            .push(SharedRuntimePlugin::new(runtime_plugin));
        self
    }

    /// Sets a typed value for a setting that only some models use.
    ///
    /// The Rust type is the key. A key stored with `StoreReplace` keeps one value, and a later
    /// `put` wins. A key stored with `StoreAppend` keeps every `put`.
    pub fn put<T: ConfigKey>(mut self, value: T) -> Self
    where
        T::Storer: sealed::Sealed<T>,
    {
        self.puts.push(Put {
            type_id: TypeId::of::<T>(),
            name: type_name::<T>(),
            applies: T::APPLIES,
            store: Arc::new(move |layer| {
                <T::Storer as sealed::Sealed<T>>::store(layer, value.clone(), sealed::Token)
            }),
        });
        self
    }

    /// Sets the AWS region.
    #[cfg(feature = "aws")]
    pub fn region(mut self, region: Region) -> Self {
        self.region = Some(region);
        self
    }

    /// Sets the provider of the AWS credentials that sign requests.
    #[cfg(feature = "aws")]
    pub fn credentials_provider(
        mut self,
        credentials_provider: impl ProvideCredentials + 'static,
    ) -> Self {
        self.credentials_provider = Some(SharedCredentialsProvider::new(credentials_provider));
        self
    }

    /// Sets the generator of the invocation ID sent with each request.
    #[cfg(feature = "aws")]
    pub fn invocation_id_generator(
        mut self,
        invocation_id_generator: impl InvocationIdGenerator + 'static,
    ) -> Self {
        self.invocation_id_generator =
            Some(SharedInvocationIdGenerator::new(invocation_id_generator));
        self
    }

    /// Sets whether clock skew correction is disabled.
    ///
    /// Clock skew correction is on by default. It adjusts the request time by the difference
    /// between the client's clock and the server's clock.
    #[cfg(feature = "aws")]
    pub fn disable_clock_skew_correction(mut self, disable_clock_skew_correction: bool) -> Self {
        self.disable_clock_skew_correction = Some(disable_clock_skew_correction);
        self
    }

    /// Builds the config.
    pub fn build(self) -> Config {
        Config { builder: self }
    }

    // Layers `later` over `self`: each field `later` sets wins, and lists append.
    pub(crate) fn merge(mut self, mut later: Builder) -> Self {
        self.retry_classifiers.append(&mut later.retry_classifiers);
        self.interceptors.append(&mut later.interceptors);
        self.runtime_plugins.append(&mut later.runtime_plugins);
        self.puts.append(&mut later.puts);
        Self {
            behavior_version: later.behavior_version.or(self.behavior_version),
            protocol: later.protocol.or(self.protocol),
            http_client: later.http_client.or(self.http_client),
            sleep_impl: later.sleep_impl.or(self.sleep_impl),
            time_source: later.time_source.or(self.time_source),
            retry_config: later.retry_config.or(self.retry_config),
            timeout_config: later.timeout_config.or(self.timeout_config),
            retry_partition: later.retry_partition.or(self.retry_partition),
            identity_cache: later.identity_cache.or(self.identity_cache),
            stalled_stream_protection: later
                .stalled_stream_protection
                .or(self.stalled_stream_protection),
            retry_classifiers: self.retry_classifiers,
            interceptors: self.interceptors,
            runtime_plugins: self.runtime_plugins,
            puts: self.puts,
            #[cfg(feature = "aws")]
            region: later.region.or(self.region),
            #[cfg(feature = "aws")]
            credentials_provider: later.credentials_provider.or(self.credentials_provider),
            #[cfg(feature = "aws")]
            invocation_id_generator: later
                .invocation_id_generator
                .or(self.invocation_id_generator),
            #[cfg(feature = "aws")]
            disable_clock_skew_correction: later
                .disable_clock_skew_correction
                .or(self.disable_clock_skew_correction),
        }
    }
}

// A value set by `put`. At `build()`, the value is written into a layer by `store`, and the key
// is checked against the service.
#[derive(Clone)]
pub(crate) struct Put {
    pub(crate) type_id: TypeId,
    pub(crate) name: &'static str,
    pub(crate) applies: &'static [Applies],
    pub(crate) store: Arc<dyn Fn(&mut Layer) + Send + Sync>,
}

impl fmt::Debug for Put {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Put")
            .field("name", &self.name)
            .field("applies", &self.applies)
            .finish_non_exhaustive()
    }
}

// How `put` stores a key: `StoreReplace` keys with `store_put`, and `StoreAppend` keys with
// `store_append`. The trait is sealed, so new store kinds can be added without a breaking change.
mod sealed {
    use aws_smithy_types::config_bag::{Layer, Storable, StoreAppend, StoreReplace};

    pub trait Sealed<T> {
        fn store(layer: &mut Layer, value: T, _token: Token);
    }

    impl<T: Storable<Storer = StoreReplace<T>>> Sealed<T> for StoreReplace<T> {
        fn store(layer: &mut Layer, value: T, _token: Token) {
            layer.store_put(value);
        }
    }

    impl<T: Storable<Storer = StoreAppend<T>>> Sealed<T> for StoreAppend<T> {
        fn store(layer: &mut Layer, value: T, _token: Token) {
            layer.store_append(value);
        }
    }

    // A public type in a private module: no other crate can name it, so no other crate can
    // create one or call `store`.
    #[derive(Debug)]
    pub struct Token;
}

impl From<Config> for Builder {
    fn from(config: Config) -> Self {
        config.builder
    }
}

/// Copies the settings of an [`SdkConfig`] into a builder.
///
/// A setting the `SdkConfig` leaves unset stays unset in the builder. So a builder made from an
/// `SdkConfig` can be layered over another config with
/// [`config`](crate::client::DynamicClientBuilder::config), and only the settings the `SdkConfig`
/// sets win.
#[cfg(feature = "aws")]
impl From<&SdkConfig> for Builder {
    // TODO(dynamic-client): Copy the other `SdkConfig` settings, such as `endpoint_url`,
    // `use_fips`, `use_dual_stack`, and `app_name`, when the config gets setters for them.
    fn from(sdk_config: &SdkConfig) -> Self {
        Builder {
            behavior_version: sdk_config.behavior_version(),
            protocol: sdk_config.protocol(),
            http_client: sdk_config.http_client(),
            sleep_impl: sdk_config.sleep_impl(),
            time_source: sdk_config.time_source(),
            retry_config: sdk_config.retry_config().cloned(),
            timeout_config: sdk_config.timeout_config().cloned(),
            identity_cache: sdk_config.identity_cache(),
            stalled_stream_protection: sdk_config.stalled_stream_protection(),
            region: sdk_config.region().cloned(),
            credentials_provider: sdk_config.credentials_provider(),
            disable_clock_skew_correction: sdk_config.disable_clock_skew_correction(),
            ..Default::default()
        }
    }
}
