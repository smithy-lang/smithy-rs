/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Load timeout configuration properties from an AWS profile

use aws_smithy_types::timeout::{TimeoutConfigBuilder, TimeoutConfigError};
use aws_types::os_shim_internal::Env;
use std::time::Duration;

const ENV_VAR_CONNECT_TIMEOUT: &str = "AWS_CONNECT_TIMEOUT";
const ENV_VAR_TLS_NEGOTIATION_TIMEOUT: &str = "AWS_TLS_NEGOTIATION_TIMEOUT";
const ENV_VAR_READ_TIMEOUT: &str = "AWS_READ_TIMEOUT";
const ENV_VAR_API_CALL_ATTEMPT_TIMEOUT: &str = "AWS_API_CALL_ATTEMPT_TIMEOUT";
const ENV_VAR_API_CALL_TIMEOUT: &str = "AWS_API_CALL_TIMEOUT";

/// Load a timeout_config from environment variables
///
/// This provider will check the values of the following variables in order to build a `TimeoutConfig`
///
/// - AWS_CONNECT_TIMEOUT
/// - AWS_TLS_NEGOTIATION_TIMEOUT
/// - AWS_READ_TIMEOUT
/// - AWS_API_CALL_ATTEMPT_TIMEOUT
/// - AWS_API_CALL_TIMEOUT
///
/// If at least one of these is set to a valid value, construction will succeed.
#[derive(Debug, Default)]
pub struct EnvironmentVariableTimeoutConfigProvider {
    env: Env,
}

impl EnvironmentVariableTimeoutConfigProvider {
    /// Create a new `EnvironmentVariableTimeoutConfigProvider`
    pub fn new() -> Self {
        EnvironmentVariableTimeoutConfigProvider { env: Env::real() }
    }

    #[doc(hidden)]
    /// Create an timeout_config provider from a given `Env`
    ///
    /// This method is used for tests that need to override environment variables.
    pub fn new_with_env(env: Env) -> Self {
        EnvironmentVariableTimeoutConfigProvider { env }
    }

    /// Attempt to create a new `TimeoutConfig` from environment variables
    pub fn timeout_config_builder(&self) -> Result<TimeoutConfigBuilder, TimeoutConfigError> {
        let connect_timeout = construct_timeout_from_env_var(&self.env, ENV_VAR_CONNECT_TIMEOUT)?
            .map(Duration::from_secs_f32);
        let tls_negotiation_timeout =
            construct_timeout_from_env_var(&self.env, ENV_VAR_TLS_NEGOTIATION_TIMEOUT)?
                .map(Duration::from_secs_f32);
        let read_timeout = construct_timeout_from_env_var(&self.env, ENV_VAR_READ_TIMEOUT)?
            .map(Duration::from_secs_f32);
        let api_call_attempt_timeout =
            construct_timeout_from_env_var(&self.env, ENV_VAR_API_CALL_ATTEMPT_TIMEOUT)?
                .map(Duration::from_secs_f32);
        let api_call_timeout = construct_timeout_from_env_var(&self.env, ENV_VAR_API_CALL_TIMEOUT)?
            .map(Duration::from_secs_f32);

        let mut builder = TimeoutConfigBuilder::new();
        builder
            .set_connect_timeout(connect_timeout)
            .set_tls_negotiation_timeout(tls_negotiation_timeout)
            .set_read_timeout(read_timeout)
            .set_api_call_attempt_timeout(api_call_attempt_timeout)
            .set_api_call_timeout(api_call_timeout);

        Ok(builder)
    }
}

const SET_BY: &str = "environment variable";

fn construct_timeout_from_env_var(env: &Env, var: &str) -> Result<Option<f32>, TimeoutConfigError> {
    // TODO do I really need to clone this?
    let var = var.to_owned();
    match env.get(&var).ok() {
        Some(timeout) => match timeout.parse::<f32>() {
            Ok(timeout) if timeout < 0.0 => Err(TimeoutConfigError::InvalidTimeout {
                set_by: SET_BY.into(),
                name: var.into(),
                reason: "timeout must not be negative".into(),
            }),
            Ok(timeout) if timeout.is_nan() => Err(TimeoutConfigError::InvalidTimeout {
                set_by: SET_BY.into(),
                name: var.into(),
                reason: "timeout must not be NaN".into(),
            }),
            Ok(timeout) if timeout.is_infinite() => Err(TimeoutConfigError::InvalidTimeout {
                set_by: SET_BY.into(),
                name: var.into(),
                reason: "timeout must not be infinite".into(),
            }),
            Ok(timeout) => Ok(Some(timeout)),
            Err(_) => Err(TimeoutConfigError::CouldntParseTimeout {
                set_by: SET_BY.into(),
                name: var.into(),
            }),
        },
        None => Ok(None),
    }
}

#[cfg(test)]
mod test {
    use super::{
        EnvironmentVariableTimeoutConfigProvider, ENV_VAR_API_CALL_ATTEMPT_TIMEOUT,
        ENV_VAR_API_CALL_TIMEOUT, ENV_VAR_CONNECT_TIMEOUT, ENV_VAR_READ_TIMEOUT,
        ENV_VAR_TLS_NEGOTIATION_TIMEOUT, SET_BY,
    };
    use aws_smithy_types::timeout::{TimeoutConfigBuilder, TimeoutConfigError};
    use aws_types::os_shim_internal::Env;
    use std::time::Duration;

    fn test_provider(vars: &[(&str, &str)]) -> EnvironmentVariableTimeoutConfigProvider {
        EnvironmentVariableTimeoutConfigProvider::new_with_env(Env::from_slice(vars))
    }

    #[test]
    fn no_defaults() {
        let built = test_provider(&[]).timeout_config_builder().unwrap().build();

        assert_eq!(built.read_timeout(), None);
        assert_eq!(built.connect_timeout(), None);
        assert_eq!(built.tls_negotiation_timeout(), None);
        assert_eq!(built.api_call_attempt_timeout(), None);
        assert_eq!(built.api_call_timeout(), None);
    }

    #[test]
    fn integer_timeout_is_read_correctly() {
        assert_eq!(
            test_provider(&[(ENV_VAR_CONNECT_TIMEOUT, "8")])
                .timeout_config_builder()
                .unwrap()
                .build(),
            TimeoutConfigBuilder::new()
                .connect_timeout(Duration::from_secs_f32(8.0))
                .build()
        );
    }

    #[test]
    fn timeout_errors_when_it_cant_be_parsed_as_a_float() {
        assert!(matches!(
            test_provider(&[(ENV_VAR_READ_TIMEOUT, "not a float")])
                .timeout_config_builder()
                .unwrap_err(),
            TimeoutConfigError::CouldntParseTimeout { .. }
        ));
    }

    #[test]
    fn all_fields_can_be_set_at_once() {
        assert_eq!(
            test_provider(&[
                (ENV_VAR_READ_TIMEOUT, "1.0"),
                (ENV_VAR_CONNECT_TIMEOUT, "2"),
                (ENV_VAR_TLS_NEGOTIATION_TIMEOUT, "3.0000"),
                (ENV_VAR_API_CALL_ATTEMPT_TIMEOUT, "04.000"),
                (ENV_VAR_API_CALL_TIMEOUT, "900012345.0")
            ])
            .timeout_config_builder()
            .unwrap()
            .build(),
            TimeoutConfigBuilder::new()
                .read_timeout(Duration::from_secs_f32(1.0))
                .connect_timeout(Duration::from_secs_f32(2.0))
                .tls_negotiation_timeout(Duration::from_secs_f32(3.0))
                .api_call_attempt_timeout(Duration::from_secs_f32(4.0))
                // Some floats can't be represented as f32 so this duration will be equal to the
                // duration from the env.
                .api_call_timeout(Duration::from_secs_f32(900012350.0))
                .build()
        );
    }

    #[test]
    fn disallow_negative_timeouts() {
        let err = test_provider(&[(ENV_VAR_TLS_NEGOTIATION_TIMEOUT, "-1.0")])
            .timeout_config_builder()
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            TimeoutConfigError::InvalidTimeout {
                set_by: SET_BY.into(),
                name: ENV_VAR_TLS_NEGOTIATION_TIMEOUT.into(),
                reason: "timeout must not be negative".into(),
            }
            .to_string()
        );
    }

    #[test]
    fn disallow_nan_timeouts() {
        let err = test_provider(&[(ENV_VAR_API_CALL_TIMEOUT, "NaN")])
            .timeout_config_builder()
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            TimeoutConfigError::InvalidTimeout {
                set_by: SET_BY.into(),
                name: ENV_VAR_API_CALL_TIMEOUT.into(),
                reason: "timeout must not be NaN".into(),
            }
            .to_string()
        );
    }

    #[test]
    fn disallow_infinite_timeouts() {
        let err = test_provider(&[
            // Infinities can be negative but that case is covered by [disallow_negative_timeouts]
            (ENV_VAR_API_CALL_ATTEMPT_TIMEOUT, "inf"),
        ])
        .timeout_config_builder()
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            TimeoutConfigError::InvalidTimeout {
                set_by: SET_BY.into(),
                name: ENV_VAR_API_CALL_ATTEMPT_TIMEOUT.into(),
                reason: "timeout must not be infinite".into(),
            }
            .to_string()
        );
    }
}
