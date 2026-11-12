/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Configuration for a dynamic client.

pub use aws_smithy_runtime_api::client::behavior_version::BehaviorVersion;

use aws_smithy_async::rt::sleep::{AsyncSleep, SharedAsyncSleep};
use aws_smithy_runtime_api::client::http::{HttpClient, SharedHttpClient};
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

    /// Builds the config.
    pub fn build(self) -> Config {
        Config { builder: self }
    }

    // Layers `later` over `self`: each field `later` sets wins.
    pub(crate) fn merge(self, later: Builder) -> Self {
        Self {
            behavior_version: later.behavior_version.or(self.behavior_version),
            protocol: later.protocol.or(self.protocol),
            http_client: later.http_client.or(self.http_client),
            sleep_impl: later.sleep_impl.or(self.sleep_impl),
        }
    }
}

impl From<Config> for Builder {
    fn from(config: Config) -> Self {
        config.builder
    }
}
