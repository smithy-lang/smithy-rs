/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::provider_config::ProviderConfig;
use aws_runtime::env_config::EnvConfigValue;
use aws_smithy_types::error::display::DisplayErrorContext;
use aws_types::sdk_config::RequestChecksumCalculation;
use std::str::FromStr;

mod env {
    pub(super) const REQUEST_CHECKSUM_CALCULATION: &str = "AWS_REQUEST_CHECKSUM_CALCULATION";
}

mod profile_key {
    pub(super) const REQUEST_CHECKSUM_CALCULATION: &str = "request_checksum_calculation";
}

/// Load the value for `request_checksum_calculation`
///
/// This checks the following sources:
/// 1. The environment variable `AWS_REQUEST_CHECKSUM_CALCULATION=WHEN_SUPPORTED/WHEN_REQUIRED`
/// 2. The profile key `request_checksum_calculation=WHEN_SUPPORTED/WHEN_REQUIRED`
///
/// If invalid values are found, the provider will return `None` and an error will be logged.
pub async fn request_checksum_calculation_provider(
    provider_config: &ProviderConfig,
) -> Option<RequestChecksumCalculation> {
    let env = provider_config.env();
    let profiles = provider_config.profile().await;

    let loaded = EnvConfigValue::new()
         .env(env::REQUEST_CHECKSUM_CALCULATION)
         .profile(profile_key::REQUEST_CHECKSUM_CALCULATION)
         .validate(&env, profiles, RequestChecksumCalculation::from_str)
         .map_err(
             |err| tracing::warn!(err = %DisplayErrorContext(&err), "invalid value for request_checksum_calculation setting"),
         )
         // WhenSupported is the default setting
         .unwrap_or(Some(RequestChecksumCalculation::WhenSupported));

    // request_checksum_calculation should always have a non-None value
    loaded.xor(Some(RequestChecksumCalculation::WhenSupported))
}

#[cfg(test)]
mod test {
    use crate::default_provider::request_checksum_calculation::request_checksum_calculation_provider;
    #[allow(deprecated)]
    use crate::profile::profile_file::{ProfileFileKind, ProfileFiles};
    use crate::provider_config::ProviderConfig;
    use aws_types::os_shim_internal::{Env, Fs};
    use aws_types::sdk_config::RequestChecksumCalculation;
    use tracing_test::traced_test;

    #[tokio::test]
    #[traced_test]
    async fn log_error_on_invalid_value() {
        let conf = ProviderConfig::empty().with_env(Env::from_slice(&[(
            "AWS_REQUEST_CHECKSUM_CALCULATION",
            "not-a-valid-value",
        )]));
        assert_eq!(request_checksum_calculation_provider(&conf).await, None);
        assert!(logs_contain(
            "invalid value for request_checksum_calculation setting"
        ));
        assert!(logs_contain("AWS_REQUEST_CHECKSUM_CALCULATION"));
    }

    #[tokio::test]
    #[traced_test]
    async fn environment_priority() {
        let conf = ProviderConfig::empty()
            .with_env(Env::from_slice(&[(
                "AWS_REQUEST_CHECKSUM_CALCULATION",
                "WHEN_SUPPORTED",
            )]))
            .with_profile_config(
                Some(
                    #[allow(deprecated)]
                    ProfileFiles::builder()
                        .with_file(
                            #[allow(deprecated)]
                            ProfileFileKind::Config,
                            "conf",
                        )
                        .build(),
                ),
                None,
            )
            .with_fs(Fs::from_slice(&[(
                "conf",
                "[default]\nrequest_checksum_calculation = WHEN_REQUIRED",
            )]));
        assert_eq!(
            request_checksum_calculation_provider(&conf).await,
            Some(RequestChecksumCalculation::WhenSupported)
        );
    }

    #[tokio::test]
    #[traced_test]
    async fn profile_works() {
        let conf = ProviderConfig::empty()
            .with_profile_config(
                Some(
                    #[allow(deprecated)]
                    ProfileFiles::builder()
                        .with_file(
                            #[allow(deprecated)]
                            ProfileFileKind::Config,
                            "conf",
                        )
                        .build(),
                ),
                None,
            )
            .with_fs(Fs::from_slice(&[(
                "conf",
                "[default]\nrequest_checksum_calculation = WHEN_REQUIRED",
            )]));
        assert_eq!(
            request_checksum_calculation_provider(&conf).await,
            Some(RequestChecksumCalculation::WhenRequired)
        );
    }
}
