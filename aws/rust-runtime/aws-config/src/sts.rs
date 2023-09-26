/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Credential provider augmentation through the AWS Security Token Service (STS).

use aws_sdk_sts::config::Builder as StsConfigBuilder;
use aws_smithy_types::retry::RetryConfig;

pub use assume_role::{AssumeRoleProvider, AssumeRoleProviderBuilder};

mod assume_role;
pub(crate) mod util;

impl crate::provider_config::ProviderConfig {
    pub(crate) fn sts_client_config(&self) -> StsConfigBuilder {
        let mut builder = aws_sdk_sts::Config::builder()
            .retry_config(RetryConfig::standard())
            .region(self.region())
            .time_source(self.time_source());
        builder.set_http_client(self.http_client());
        builder.set_sleep_impl(self.sleep_impl());
        builder
    }
}
