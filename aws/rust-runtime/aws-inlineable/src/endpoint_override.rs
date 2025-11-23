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
use aws_types::endpoint_config::EndpointUrl;

/// Interceptor that tracks AWS SDK features for endpoint override.
#[derive(Debug, Default)]
pub(crate) struct EndpointOverrideFeatureTrackerInterceptor;

impl Intercept for EndpointOverrideFeatureTrackerInterceptor {
    fn name(&self) -> &'static str {
        "EndpointOverrideFeatureTrackerInterceptor"
    }

    fn read_before_execution(
        &self,
        _context: &BeforeSerializationInterceptorContextRef<'_>,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        if cfg.load::<EndpointUrl>().is_some() {
            cfg.interceptor_state()
                .store_append(AwsSdkFeature::EndpointOverride);
        }
        Ok(())
    }
}
