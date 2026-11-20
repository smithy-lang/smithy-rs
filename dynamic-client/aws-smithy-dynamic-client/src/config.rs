/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Configuration for a dynamic client.

pub use aws_smithy_dynamic_client_api::config_key::{Applies, ConfigKey, EndpointBuiltIn};

pub use aws_smithy_runtime::client::identity::IdentityCache;
pub use aws_smithy_runtime::client::retries::RetryPartition;
pub use aws_smithy_runtime_api::client::auth::{AuthSchemeId, AuthSchemePreference};
pub use aws_smithy_runtime_api::client::behavior_version::BehaviorVersion;
pub use aws_smithy_runtime_api::client::stalled_stream_protection::StalledStreamProtectionConfig;
pub use aws_smithy_types::retry::RetryConfig;
pub use aws_smithy_types::timeout::TimeoutConfig;
#[cfg(feature = "aws")]
pub use aws_types::region::Region;

use aws_smithy_async::rt::sleep::{AsyncSleep, SharedAsyncSleep};
use aws_smithy_async::time::{SharedTimeSource, TimeSource};
use aws_smithy_runtime::client::auth::no_auth::NoAuthRuntimePluginV2;
use aws_smithy_runtime_api::client::auth::{
    ResolveAuthSchemeOptions, SharedAuthSchemeOptionResolver,
};
use aws_smithy_runtime_api::client::http::{HttpClient, SharedHttpClient};
use aws_smithy_runtime_api::client::identity::{ResolveCachedIdentity, SharedIdentityCache};
use aws_smithy_runtime_api::client::interceptors::{Intercept, SharedInterceptor};
use aws_smithy_runtime_api::client::retries::classifiers::{ClassifyRetry, SharedRetryClassifier};
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
use aws_smithy_runtime_api::client::runtime_plugin::{
    RuntimePlugin, SharedRuntimePlugin, StaticRuntimePlugin,
};
use aws_smithy_runtime_api::shared::IntoShared;
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

pub mod auth {
    //! The parameters an auth scheme resolver gets.

    pub use crate::auth::Params;
}

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
    pub(crate) auth_scheme_resolver: Option<SharedAuthSchemeOptionResolver>,
    pub(crate) auth_scheme_preference: Option<AuthSchemePreference>,
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

    /// Sets the auth scheme resolver. For each call, the resolver returns the auth schemes to
    /// try, in order.
    ///
    /// Without one, each call tries the operation's auth schemes from the model, as listed by
    /// [`Params::auth_schemes`](auth::Params::auth_schemes). A resolver gets the same
    /// [`auth::Params`]:
    ///
    /// ```
    /// use aws_smithy_dynamic_client::config::auth::Params;
    /// use aws_smithy_dynamic_client::config::Config;
    /// use aws_smithy_runtime_api::client::auth::{
    ///     AuthSchemeOption, AuthSchemeOptionResolverParams, AuthSchemeOptionsFuture,
    ///     ResolveAuthSchemeOptions,
    /// };
    /// use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
    /// use aws_smithy_types::config_bag::ConfigBag;
    ///
    /// // Tries the operation's auth schemes in reverse order.
    /// #[derive(Debug)]
    /// struct Reversed;
    ///
    /// impl ResolveAuthSchemeOptions for Reversed {
    ///     fn resolve_auth_scheme_options_v2<'a>(
    ///         &'a self,
    ///         params: &'a AuthSchemeOptionResolverParams,
    ///         _cfg: &'a ConfigBag,
    ///         _components: &'a RuntimeComponents,
    ///     ) -> AuthSchemeOptionsFuture<'a> {
    ///         let params = params.get::<Params>().expect("a dynamic client call");
    ///         let options = params.auth_schemes().iter().rev().cloned();
    ///         AuthSchemeOptionsFuture::ready(Ok(options.map(AuthSchemeOption::from).collect()))
    ///     }
    /// }
    ///
    /// let config = Config::builder().auth_scheme_resolver(Reversed).build();
    /// ```
    pub fn auth_scheme_resolver(
        mut self,
        auth_scheme_resolver: impl ResolveAuthSchemeOptions + 'static,
    ) -> Self {
        self.auth_scheme_resolver = Some(auth_scheme_resolver.into_shared());
        self
    }

    /// Sets the auth scheme preference. Each call tries the preferred auth schemes first, in the
    /// preference's order, and then the rest of its auth schemes.
    ///
    /// The preference only reorders the auth schemes of a call. A preferred scheme that the call
    /// does not have is ignored.
    ///
    /// ```
    /// use aws_smithy_dynamic_client::config::Config;
    /// use aws_smithy_runtime_api::client::auth::http::HTTP_BEARER_AUTH_SCHEME_ID;
    ///
    /// let config = Config::builder()
    ///     .auth_scheme_preference([HTTP_BEARER_AUTH_SCHEME_ID])
    ///     .build();
    /// ```
    pub fn auth_scheme_preference(
        mut self,
        auth_scheme_preference: impl Into<AuthSchemePreference>,
    ) -> Self {
        self.auth_scheme_preference = Some(auth_scheme_preference.into());
        self
    }

    /// Allows calls with no auth.
    ///
    /// Each call tries `noAuth` after the auth schemes of its operation. So a call with no
    /// identity for any of those schemes is sent with no auth.
    pub fn allow_no_auth(mut self) -> Self {
        self.runtime_plugins
            .push(NoAuthRuntimePluginV2::new().into_shared());
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
            // A later timeout config keeps each timeout it leaves unset from the earlier one, as
            // generated clients merge timeout configs.
            timeout_config: match (later.timeout_config, self.timeout_config) {
                (Some(mut later), Some(earlier)) => {
                    later.take_defaults_from(&earlier);
                    Some(later)
                }
                (later, earlier) => later.or(earlier),
            },
            retry_partition: later.retry_partition.or(self.retry_partition),
            identity_cache: later.identity_cache.or(self.identity_cache),
            auth_scheme_resolver: later.auth_scheme_resolver.or(self.auth_scheme_resolver),
            auth_scheme_preference: later.auth_scheme_preference.or(self.auth_scheme_preference),
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

    // Splits the config into the parts generated clients store the config in. The behavior
    // version and the `put` values are left out, because a client stores them apart. A
    // protocol, when one is set, is stored in the layer.
    pub(crate) fn into_parts(self, name: &'static str) -> ConfigParts {
        let mut components = RuntimeComponentsBuilder::new(name);
        components.set_http_client(self.http_client);
        components.set_sleep_impl(self.sleep_impl);
        components.set_time_source(self.time_source);
        components.set_identity_cache(self.identity_cache);
        components.set_auth_scheme_option_resolver(self.auth_scheme_resolver);
        for retry_classifier in self.retry_classifiers {
            components.push_retry_classifier(retry_classifier);
        }
        for interceptor in self.interceptors {
            components.push_interceptor(interceptor);
        }
        // TODO(dynamic-client): Also set the credentials provider for SigV4a, for a model that
        // uses SigV4a, as generated clients do.
        #[cfg(feature = "aws")]
        if let Some(credentials_provider) = self.credentials_provider {
            components
                .set_identity_resolver(aws_runtime::auth::sigv4::SCHEME_ID, credentials_provider);
        }
        let mut layer = Layer::new(name);
        if let Some(protocol) = self.protocol {
            layer.store_put(protocol);
        }
        if let Some(retry_config) = self.retry_config {
            layer.store_put(retry_config);
        }
        if let Some(timeout_config) = self.timeout_config {
            layer.store_put(timeout_config);
        }
        if let Some(retry_partition) = self.retry_partition {
            layer.store_put(retry_partition);
        }
        if let Some(stalled_stream_protection) = self.stalled_stream_protection {
            layer.store_put(stalled_stream_protection);
        }
        if let Some(auth_scheme_preference) = self.auth_scheme_preference {
            layer.store_put(auth_scheme_preference);
        }
        #[cfg(feature = "aws")]
        if let Some(region) = self.region {
            layer.store_put(region);
        }
        #[cfg(feature = "aws")]
        if let Some(invocation_id_generator) = self.invocation_id_generator {
            layer.store_put(invocation_id_generator);
        }
        #[cfg(feature = "aws")]
        if let Some(disable_clock_skew_correction) = self.disable_clock_skew_correction {
            layer.store_put(
                aws_runtime::service_clock_skew::DisableClockSkewCorrection::from(
                    disable_clock_skew_correction,
                ),
            );
        }
        ConfigParts {
            layer,
            components,
            runtime_plugins: self.runtime_plugins,
        }
    }
}

// `ConfigParts` holds the parts of a config: a config layer, runtime components, and the user's
// runtime plugins.
pub(crate) struct ConfigParts {
    pub(crate) layer: Layer,
    pub(crate) components: RuntimeComponentsBuilder,
    pub(crate) runtime_plugins: Vec<SharedRuntimePlugin>,
}

// Returns a plugin with the values set by `put`, in the order they were set. Returns `None` when
// no value was set.
pub(crate) fn puts_plugin(puts: &[Put]) -> Option<StaticRuntimePlugin> {
    if puts.is_empty() {
        return None;
    }
    let mut layer = Layer::new("DynamicClientPuts");
    for put in puts {
        (put.store)(&mut layer);
    }
    Some(StaticRuntimePlugin::new().with_config(layer.freeze()))
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
            auth_scheme_preference: sdk_config.auth_scheme_preference().cloned(),
            region: sdk_config.region().cloned(),
            credentials_provider: sdk_config.credentials_provider(),
            disable_clock_skew_correction: sdk_config.disable_clock_skew_correction(),
            ..Default::default()
        }
    }
}
