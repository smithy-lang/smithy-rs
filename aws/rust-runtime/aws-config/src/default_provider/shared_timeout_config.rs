/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::environment::shared_timeout_config::EnvironmentVariableSharedTimeoutConfigProvider;
use crate::profile;
use crate::provider_config::ProviderConfig;

use aws_smithy_types::timeout::SharedTimeoutConfig;

/// Default [`SharedTimeoutConfig`] Provider chain
///
/// Unlike other credentials and region, [`SharedTimeoutConfig`] has no related `SharedTimeoutConfigProvider` trait. Instead,
/// a builder struct is returned which has a similar API.
///
/// This provider will check the following sources in order:
/// 1. [Environment variables](EnvironmentVariableSharedTimeoutConfigProvider)
/// 2. [Profile file](crate::profile::timeout_config::ProfileFileSharedTimeoutConfigProvider) (`~/.aws/config`)
///
/// # Example
///
/// ```no_run
/// # use std::error::Error;
/// # #[tokio::main]
/// # async fn main() {
/// use aws_config::default_provider::shared_timeout_config;
///
/// // Load a timeout config from a specific profile
/// let timeout_config = shared_timeout_config::default_provider()
///     .profile_name("other_profile")
///     .timeout_config()
///     .await;
/// let config = aws_config::from_env()
///     // Override the timeout config set by the default profile
///     .timeout_config(timeout_config)
///     .load()
///     .await;
/// // instantiate a service client:
/// // <my_aws_service>::Client::new(&config);
/// # }
/// ```
pub fn default_provider() -> Builder {
    Builder::default()
}

/// Builder for [`TimeoutConfig`] that checks the environment variables and AWS profile files for configuration
#[derive(Default)]
pub struct Builder {
    env_provider: EnvironmentVariableSharedTimeoutConfigProvider,
    profile_file: profile::shared_timeout_config::Builder,
}

impl Builder {
    /// Configure the default chain
    ///
    /// Exposed for overriding the environment when unit-testing providers
    pub fn configure(mut self, configuration: &ProviderConfig) -> Self {
        self.env_provider =
            EnvironmentVariableSharedTimeoutConfigProvider::new_with_env(configuration.env());
        self.profile_file = self.profile_file.configure(configuration);
        self
    }

    /// Override the profile name used by this provider
    pub fn profile_name(mut self, name: &str) -> Self {
        self.profile_file = self.profile_file.profile_name(name);
        self
    }

    /// Attempt to create a [`TimeoutConfig`](aws_smithy_types::timeout::TimeoutConfig) from following sources in order:
    /// 1. [Environment variables](crate::environment::timeout_config::EnvironmentVariableTimeoutConfigProvider)
    /// 2. [Profile file](crate::profile::timeout_config::ProfileFileTimeoutConfigProvider)
    ///
    /// Precedence is considered on a per-field basis. If no timeout is specified, requests will never time out.
    ///
    /// # Panics
    ///
    /// This will panic if:
    /// - a timeout is set to `NaN`, a negative number, or infinity
    /// - a timeout can't be parsed as a floating point number
    pub async fn timeout_config(self) -> SharedTimeoutConfig {
        // Both of these can return errors due to invalid config settings and we want to surface those as early as possible
        // hence, we'll panic if any config values are invalid (missing values are OK though)
        // We match this instead of unwrapping so we can print the error with the `Display` impl instead of the `Debug` impl that unwrap uses
        let builder_from_env = match self.env_provider.timeout_config() {
            Ok(timeout_config_builder) => timeout_config_builder,
            Err(err) => panic!("{}", err),
        };
        let builder_from_profile = match self.profile_file.build().timeout_config().await {
            Ok(timeout_config_builder) => timeout_config_builder,
            Err(err) => panic!("{}", err),
        };

        let conf = builder_from_env.take_unset_from(builder_from_profile);

        conf
    }
}
