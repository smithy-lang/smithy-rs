/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Load timeout configuration properties from environment variables

use crate::parsing::parse_str_as_timeout;
use aws_sdk_sso::config::{TimeoutConfig, TimeoutConfigBuilder};
use aws_smithy_types::timeout;
use aws_types::os_shim_internal::Env;
use std::time::Duration;

// Currently unsupported timeouts
const ENV_VAR_TLS_NEGOTIATION_TIMEOUT: &str = "AWS_TLS_NEGOTIATION_TIMEOUT";

// Supported timeouts
const ENV_VAR_CONNECT_TIMEOUT: &str = "AWS_CONNECT_TIMEOUT";
const ENV_VAR_READ_TIMEOUT: &str = "AWS_READ_TIMEOUT";
const ENV_VAR_OPERATION_ATTEMPT_TIMEOUT: &str = "AWS_OPERATION_ATTEMPT_TIMEOUT";
const ENV_VAR_OPERATION_TIMEOUT: &str = "AWS_OPERATION_TIMEOUT";

/// Load timeout config from environment variables
///
/// This provider will check the values of the following variables in order to build a [`TimeoutConfig`]:
///
/// - `AWS_CONNECT_TIMEOUT`
/// - `AWS_READ_TIMEOUT`
/// - `AWS_OPERATION_ATTEMPT_TIMEOUT`
/// - `AWS_OPERATION_TIMEOUT`
///
/// Timeout values represent the number of seconds before timing out and must be non-negative floats
/// or integers. NaN and infinity are also invalid.
#[derive(Debug, Default)]
pub struct EnvironmentVariableTimeoutConfigProvider {
    env: Env,
}

impl EnvironmentVariableTimeoutConfigProvider {
    /// Create a new [`EnvironmentVariableTimeoutConfigProvider`]
    pub fn new() -> Self {
        EnvironmentVariableTimeoutConfigProvider { env: Env::real() }
    }

    #[doc(hidden)]
    /// Create a timeout config provider from a given [`Env`]
    ///
    /// This method is used for tests that need to override environment variables.
    pub fn new_with_env(env: Env) -> Self {
        EnvironmentVariableTimeoutConfigProvider { env }
    }

    /// Attempt to create a new [`timeout::Config`](aws_smithy_types::timeout::Config) from environment variables
    pub fn timeout_config(&self) -> Result<TimeoutConfigBuilder, timeout::ConfigError> {
        // Warn users that set unsupported timeouts in their profile
        for timeout in [ENV_VAR_TLS_NEGOTIATION_TIMEOUT] {
            warn_if_unsupported_timeout_is_set(&self.env, timeout);
        }

        let mut builder = TimeoutConfig::builder();
        builder.set_connect_timeout(timeout_from_env_var(&self.env, ENV_VAR_CONNECT_TIMEOUT)?);
        builder.set_read_timeout(timeout_from_env_var(&self.env, ENV_VAR_READ_TIMEOUT)?);
        builder.set_operation_attempt_timeout(timeout_from_env_var(
            &self.env,
            ENV_VAR_OPERATION_ATTEMPT_TIMEOUT,
        )?);
        builder.set_operation_timeout(timeout_from_env_var(&self.env, ENV_VAR_OPERATION_TIMEOUT)?);

        Ok(builder)
    }
}

fn timeout_from_env_var(
    env: &Env,
    var: &'static str,
) -> Result<Option<Duration>, timeout::ConfigError> {
    match env.get(var).ok() {
        Some(timeout) => {
            parse_str_as_timeout(&timeout, var.into(), "environment variable".into()).map(Some)
        }
        None => Ok(None),
    }
}

fn warn_if_unsupported_timeout_is_set(env: &Env, var: &'static str) {
    if env.get(var).is_ok() {
        tracing::warn!(
                "Environment variable for '{}' timeout was set but that feature is currently unimplemented so the setting will be ignored. \
                To help us prioritize support for this feature, please upvote aws-sdk-rust#151 (https://github.com/awslabs/aws-sdk-rust/issues/151)",
            var
        )
    }
}

#[cfg(test)]
mod test {
    use super::{
        EnvironmentVariableTimeoutConfigProvider, ENV_VAR_CONNECT_TIMEOUT,
        ENV_VAR_OPERATION_ATTEMPT_TIMEOUT, ENV_VAR_OPERATION_TIMEOUT, ENV_VAR_READ_TIMEOUT,
    };
    use aws_sdk_sso::config::TimeoutConfig;
    use aws_types::os_shim_internal::Env;
    use std::time::Duration;

    fn test_provider(vars: &[(&str, &str)]) -> EnvironmentVariableTimeoutConfigProvider {
        EnvironmentVariableTimeoutConfigProvider::new_with_env(Env::from_slice(vars))
    }

    #[test]
    fn no_defaults() {
        let built = test_provider(&[]).timeout_config().unwrap().build();
        assert_eq!(built.connect_timeout(), None);
        assert_eq!(built.read_timeout(), None);
        assert_eq!(built.operation_timeout(), None);
        assert_eq!(built.operation_attempt_timeout(), None);
    }

    #[test]
    fn all_fields_can_be_set_at_once() {
        let expected_timeouts = TimeoutConfig::builder()
            .connect_timeout(Duration::from_secs_f32(3.1))
            .read_timeout(Duration::from_secs_f32(500.0))
            .operation_attempt_timeout(Duration::from_secs_f32(4.0))
            // Some floats can't be represented as f32 so this duration will end up equalling the
            // duration from the env.
            .operation_timeout(Duration::from_secs_f32(900012350.0))
            .build();

        assert_eq!(
            test_provider(&[
                (ENV_VAR_CONNECT_TIMEOUT, "3.1"),
                (ENV_VAR_READ_TIMEOUT, "500"),
                (ENV_VAR_OPERATION_ATTEMPT_TIMEOUT, "04.000"),
                (ENV_VAR_OPERATION_TIMEOUT, "900012345.0")
            ])
            .timeout_config()
            .unwrap()
            .build(),
            expected_timeouts
        );
    }
}
