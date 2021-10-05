/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::meta::retry_config::{future, ProvideRetryConfig};
use aws_types::os_shim_internal::Env;
use smithy_types::retry::{RetryConfig, RetryMode};

/// Load a retry_config from environment variables
///
/// This provider will check the values of `AWS_RETRY_MODE` and `AWS_MAX_ATTEMPTS`
/// in order to build a retry config. If at least one is set to a valid value,
/// construction will succeed
#[derive(Debug, Default)]
pub struct EnvironmentVariableRetryConfigProvider {
    env: Env,
}

impl EnvironmentVariableRetryConfigProvider {
    /// Create a new `EnvironmentVariableRetryConfigProvider`
    pub fn new() -> Self {
        EnvironmentVariableRetryConfigProvider { env: Env::real() }
    }

    #[doc(hidden)]
    /// Create an retry_config provider from a given `Env`
    ///
    /// This method is used for tests that need to override environment variables.
    pub fn new_with_env(env: Env) -> Self {
        EnvironmentVariableRetryConfigProvider { env }
    }
}

const ENV_VAR_MAX_ATTEMPTS: &str = "AWS_MAX_ATTEMPTS";
const ENV_VAR_RETRY_MODE: &str = "AWS_RETRY_MODE";

impl ProvideRetryConfig for EnvironmentVariableRetryConfigProvider {
    fn retry_config(&self) -> future::ProvideRetryConfig {
        let max_attempts = self
            .env
            .get(ENV_VAR_MAX_ATTEMPTS)
            .ok()
            .and_then(|max_attempts| max_attempts.parse::<u32>().ok());
        let retry_mode = self
            .env
            .get(ENV_VAR_RETRY_MODE)
            .ok()
            .and_then(|retry_mode| RetryMode::from_str(&retry_mode));

        // If neither env vars are set, we're done with this provider
        if let (None, None) = (max_attempts, retry_mode) {
            return future::ProvideRetryConfig::ready(None);
        }

        // If at least one env var is set, we create a RetryConfig based on the value(s)
        let mut retry_config = RetryConfig::new();

        if let Some(max_attempts) = max_attempts {
            retry_config = retry_config.with_max_attempts(max_attempts);
        }

        if let Some(retry_mode) = retry_mode {
            retry_config = retry_config.with_retry_mode(retry_mode);
        }

        future::ProvideRetryConfig::ready(Some(retry_config))
    }
}

#[cfg(test)]
mod test {
    use crate::environment::retry_config::EnvironmentVariableRetryConfigProvider;
    use crate::meta::retry_config::ProvideRetryConfig;
    use aws_types::os_shim_internal::Env;
    use futures_util::FutureExt;
    use smithy_types::retry::{RetryConfig, RetryMode};
    use super::{ENV_VAR_MAX_ATTEMPTS, ENV_VAR_RETRY_MODE};

    fn test_provider(vars: &[(&str, &str)]) -> EnvironmentVariableRetryConfigProvider {
        EnvironmentVariableRetryConfigProvider::new_with_env(Env::from_slice(vars))
    }

    #[test]
    fn no_retry_config() {
        assert_eq!(
            test_provider(&[])
                .retry_config()
                .now_or_never()
                .expect("no polling"),
            None
        );
    }

    #[test]
    fn max_attempts_is_read_correctly() {
        assert_eq!(
            test_provider(&[(ENV_VAR_MAX_ATTEMPTS, "88")])
                .retry_config()
                .now_or_never()
                .expect("no polling"),
            Some(RetryConfig::new().with_max_attempts(88))
        );
    }

    #[test]
    fn retry_mode_is_read_correctly() {
        assert_eq!(
            test_provider(&[(ENV_VAR_RETRY_MODE, "standard")])
                .retry_config()
                .now_or_never()
                .expect("no polling"),
            Some(RetryConfig::new().with_retry_mode(RetryMode::Standard))
        );
    }

    #[test]
    fn both_fields_can_be_set_at_once() {
        assert_eq!(
            test_provider(&[(ENV_VAR_RETRY_MODE, "adaptive"), (ENV_VAR_MAX_ATTEMPTS, "13")])
                .retry_config()
                .now_or_never()
                .expect("no polling"),
            Some(
                RetryConfig::new()
                    .with_max_attempts(13)
                    .with_retry_mode(RetryMode::Adaptive)
            )
        );
    }
}
