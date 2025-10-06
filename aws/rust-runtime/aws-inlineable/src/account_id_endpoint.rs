/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_runtime::sdk_feature::AwsSdkFeature;
use aws_smithy_runtime_api::{
    box_error::BoxError,
    client::interceptors::{context::BeforeSerializationInterceptorContextRef, Intercept},
};
use aws_smithy_types::config_bag::ConfigBag;
use aws_types::endpoint_config::AccountIdEndpointMode;

// Interceptor that tracks AWS SDK features for the account based endpoints.
#[derive(Debug, Default)]
pub(crate) struct AccountIdEndpointFeatureTrackerInterceptor;

impl Intercept for AccountIdEndpointFeatureTrackerInterceptor {
    fn name(&self) -> &'static str {
        "AccountIdEndpointFeatureTrackerInterceptor"
    }

    fn read_before_execution(
        &self,
        _context: &BeforeSerializationInterceptorContextRef<'_>,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        match cfg
            .load::<AccountIdEndpointMode>()
            .cloned()
            .unwrap_or_default()
        {
            AccountIdEndpointMode::Preferred => {
                cfg.interceptor_state()
                    .store_append(AwsSdkFeature::AccountIdModePreferred);
            }
            AccountIdEndpointMode::Required => {
                cfg.interceptor_state()
                    .store_append(AwsSdkFeature::AccountIdModeRequired);
            }
            AccountIdEndpointMode::Disabled => {
                cfg.interceptor_state()
                    .store_append(AwsSdkFeature::AccountIdModeDisabled);
            }
            otherwise => {
                ::tracing::warn!(
                    "Attempted to track an SDK feature for `{otherwise:?}`, which is not recognized in the current version of the SDK. \
                    Consider upgrading to the latest version to ensure that it is properly tracked."
                );
            }
        }

        Ok(())
    }
}
