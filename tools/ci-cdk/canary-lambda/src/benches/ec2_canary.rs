/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::{mk_canary, CanaryEnv};
use aws_config::SdkConfig;
use aws_sdk_ec2 as ec2;

mk_canary!("ec2", |sdk_config: &SdkConfig, _env: &CanaryEnv| {
    let client = ec2::Client::new(sdk_config);
    async move {
        let _ = client.describe_regions().send().await;
        Ok(())
    }
});
