/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Configuration for a dynamic client.

pub use aws_smithy_runtime::client::retries::RetryPartition;
pub use aws_smithy_runtime_api::client::behavior_version::BehaviorVersion;
pub use aws_smithy_types::retry::RetryConfig;
pub use aws_smithy_types::timeout::TimeoutConfig;

use aws_smithy_async::rt::sleep::{AsyncSleep, SharedAsyncSleep};
use aws_smithy_async::time::{SharedTimeSource, TimeSource};
use aws_smithy_runtime_api::client::http::{HttpClient, SharedHttpClient};
use aws_smithy_runtime_api::client::interceptors::{Intercept, SharedInterceptor};
use aws_smithy_runtime_api::client::retries::classifiers::{ClassifyRetry, SharedRetryClassifier};
use aws_smithy_runtime_api::client::runtime_plugin::{RuntimePlugin, SharedRuntimePlugin};
use aws_smithy_schema::protocol::{ClientProtocol, SharedClientProtocol};

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
    pub(crate) retry_classifiers: Vec<SharedRetryClassifier>,
    pub(crate) interceptors: Vec<SharedInterceptor>,
    pub(crate) runtime_plugins: Vec<SharedRuntimePlugin>,
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

    /// Builds the config.
    pub fn build(self) -> Config {
        Config { builder: self }
    }

    // Layers `later` over `self`: each field `later` sets wins, and lists append.
    pub(crate) fn merge(mut self, mut later: Builder) -> Self {
        self.retry_classifiers.append(&mut later.retry_classifiers);
        self.interceptors.append(&mut later.interceptors);
        self.runtime_plugins.append(&mut later.runtime_plugins);
        Self {
            behavior_version: later.behavior_version.or(self.behavior_version),
            protocol: later.protocol.or(self.protocol),
            http_client: later.http_client.or(self.http_client),
            sleep_impl: later.sleep_impl.or(self.sleep_impl),
            time_source: later.time_source.or(self.time_source),
            retry_config: later.retry_config.or(self.retry_config),
            timeout_config: later.timeout_config.or(self.timeout_config),
            retry_partition: later.retry_partition.or(self.retry_partition),
            retry_classifiers: self.retry_classifiers,
            interceptors: self.interceptors,
            runtime_plugins: self.runtime_plugins,
        }
    }
}

impl From<Config> for Builder {
    fn from(config: Config) -> Self {
        config.builder
    }
}
