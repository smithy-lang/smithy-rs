/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Load retry configuration properties from an AWS profile

use std::str::FromStr;

use aws_types::os_shim_internal::{Env, Fs};
use smithy_types::retry::{RetryConfig, RetryConfigErr, RetryMode};

use crate::provider_config::ProviderConfig;

/// Load retry configuration properties from a profile file
///
/// This provider will attempt to load AWS shared configuration, then read retry configuration properties
/// from the active profile.
///
/// # Examples
///
/// **Loads 2 as the `max_attempts` to make when sending a request
/// ```ini
/// [default]
/// max_attempts = 2
/// ```
///
/// **Loads `standard` as the `retry_mode` _if and only if_ the `AWS_PROFILE` environment variable is set
/// to `other`.
///
/// ```ini
/// [profile other]
/// retry_mode = standard
/// ```
///
/// This provider is part of the [default retry_config provider chain](crate::default_provider::retry_config).
#[derive(Debug, Default)]
pub struct ProfileFileRetryConfigProvider {
    fs: Fs,
    env: Env,
    profile_override: Option<String>,
}

/// Builder for [ProfileFileRetryConfigProvider]
#[derive(Default)]
pub struct Builder {
    config: Option<ProviderConfig>,
    profile_override: Option<String>,
}

impl Builder {
    /// Override the configuration for this provider
    pub fn configure(mut self, config: &ProviderConfig) -> Self {
        self.config = Some(config.clone());
        self
    }

    /// Override the profile name used by the [ProfileFileRetryConfigProvider]
    pub fn profile_name(mut self, profile_name: impl Into<String>) -> Self {
        self.profile_override = Some(profile_name.into());
        self
    }

    /// Build a [ProfileFileRetryConfigProvider] from this builder
    pub fn build(self) -> ProfileFileRetryConfigProvider {
        let conf = self.config.unwrap_or_default();
        ProfileFileRetryConfigProvider {
            env: conf.env(),
            fs: conf.fs(),
            profile_override: self.profile_override,
        }
    }
}

impl ProfileFileRetryConfigProvider {
    /// Create a new [ProfileFileRetryConfigProvider]
    ///
    /// To override the selected profile, set the `AWS_PROFILE` environment variable or use the [Builder].
    pub fn new() -> Self {
        Self {
            fs: Fs::real(),
            env: Env::real(),
            profile_override: None,
        }
    }

    /// [Builder] to construct a [ProfileFileRetryConfigProvider]
    pub fn builder() -> Builder {
        Builder::default()
    }

    /// Attempt to create a new RetryConfig from a profile file.
    /// Will return None if profile file contains no retry behavior configuration.
    /// Will panic if provided configuration is invalid.
    pub async fn retry_config(&self) -> Result<Option<RetryConfig>, RetryConfigErr> {
        let profile = match super::parser::load(&self.fs, &self.env).await {
            Ok(profile) => profile,
            Err(err) => {
                tracing::warn!(err = %err, "failed to parse profile");
                return Ok(None);
            }
        };

        let selected_profile = self
            .profile_override
            .as_deref()
            .unwrap_or_else(|| profile.selected_profile());
        let selected_profile = match profile.get_profile(selected_profile) {
            Some(profile) => profile,
            None => {
                tracing::warn!("failed to get selected '{}' profile", selected_profile);
                return Ok(None);
            }
        };

        let max_attempts = selected_profile
            .get("max_attempts")
            .and_then(|max_attempts| max_attempts.parse::<u32>().ok());
        let retry_mode = selected_profile.get("retry_mode");

        // If neither profile vars are set, we're done with this provider
        if let (None, None) = (&max_attempts, retry_mode) {
            return Ok(None);
        }

        // If at least one is set, we need to generate a `RetryConfig` and update it with the set value(s)
        let mut retry_config = RetryConfig::new();

        if let Some(max_attempts) = max_attempts {
            if max_attempts == 0 {
                return Err(RetryConfigErr::MaxAttemptsMustNotBeZero {
                    set_by: "aws profile".to_owned(),
                });
            };

            retry_config = retry_config.with_max_attempts(max_attempts);
        }

        if let Some(retry_mode) = retry_mode {
            match RetryMode::from_str(retry_mode) {
                Ok(retry_mode) if retry_mode == RetryMode::Adaptive => {
                    return Err(RetryConfigErr::AdaptiveModeIsNotSupported {
                        set_by: "aws profile".to_owned(),
                    });
                }
                Ok(retry_mode) => retry_config = retry_config.with_retry_mode(retry_mode),
                Err(retry_mode_err) => {
                    return Err(RetryConfigErr::InvalidRetryMode {
                        set_by: "aws profile".to_owned(),
                        source: retry_mode_err,
                    });
                }
            };
        }

        Ok(Some(retry_config))
    }
}
