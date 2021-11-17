/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Load timeout configuration properties from Environment variables

use aws_smithy_types::timeout::{TimeoutConfig, TimeoutConfigError};
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
/// - `AWS_CONNECT_TIMEOUT`
/// - `AWS_TLS_NEGOTIATION_TIMEOUT`
/// - `AWS_READ_TIMEOUT`
/// - `AWS_API_CALL_ATTEMPT_TIMEOUT`
/// - `AWS_API_CALL_TIMEOUT`
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
    /// Create an timeout_config provider from a given [`Env`]
    ///
    /// This method is used for tests that need to override environment variables.
    pub fn new_with_env(env: Env) -> Self {
        EnvironmentVariableTimeoutConfigProvider { env }
    }

    /// Attempt to create a new [`TimeoutConfig`] from environment variables
    pub fn timeout_config(&self) -> Result<TimeoutConfig, TimeoutConfigError> {
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

        Ok(TimeoutConfig::new()
            .with_connect_timeout(connect_timeout)
            .with_tls_negotiation_timeout(tls_negotiation_timeout)
            .with_read_timeout(read_timeout)
            .with_api_call_attempt_timeout(api_call_attempt_timeout)
            .with_api_call_timeout(api_call_timeout))
    }
}

const SET_BY: &str = "environment variable";

fn construct_timeout_from_env_var(
    env: &Env,
    var: &'static str,
) -> Result<Option<f32>, TimeoutConfigError> {
    match env.get(var).ok() {
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
            Err(err) => Err(TimeoutConfigError::ParseError {
                set_by: SET_BY.into(),
                name: var.into(),
                source: Box::new(err),
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
    use aws_smithy_types::timeout::{TimeoutConfig, TimeoutConfigError};
    use aws_types::os_shim_internal::Env;
    use std::time::Duration;

    fn test_provider(vars: &[(&str, &str)]) -> EnvironmentVariableTimeoutConfigProvider {
        EnvironmentVariableTimeoutConfigProvider::new_with_env(Env::from_slice(vars))
    }

    #[test]
    fn no_defaults() {
        let built = test_provider(&[]).timeout_config().unwrap();

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
                .timeout_config()
                .unwrap(),
            TimeoutConfig::new().with_connect_timeout(Some(Duration::from_secs_f32(8.0)))
        );
    }

    #[test]
    fn timeout_errors_when_it_cant_be_parsed_as_a_float() {
        assert!(matches!(
            test_provider(&[(ENV_VAR_READ_TIMEOUT, "not a float")])
                .timeout_config()
                .unwrap_err(),
            TimeoutConfigError::ParseError { .. }
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
            .timeout_config()
            .unwrap(),
            TimeoutConfig::new()
                .with_read_timeout(Some(Duration::from_secs_f32(1.0)))
                .with_connect_timeout(Some(Duration::from_secs_f32(2.0)))
                .with_tls_negotiation_timeout(Some(Duration::from_secs_f32(3.0)))
                .with_api_call_attempt_timeout(Some(Duration::from_secs_f32(4.0)))
                // Some floats can't be represented as f32 so this duration will be equal to the
                // duration from the env.
                .with_api_call_timeout(Some(Duration::from_secs_f32(900012350.0)))
        );
    }

    #[test]
    fn disallow_negative_timeouts() {
        let err = test_provider(&[(ENV_VAR_TLS_NEGOTIATION_TIMEOUT, "-1.0")])
            .timeout_config()
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
            .timeout_config()
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
        .timeout_config()
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
