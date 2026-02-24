/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::{mk_canary, CanaryEnv};
use aws_config::SdkConfig;
use aws_sdk_sts as sts;

mk_canary!("sts", |sdk_config: &SdkConfig, _env: &CanaryEnv| {
    let client = sts::Client::new(sdk_config);
    async move {
        let _ = client.get_caller_identity().send().await;
        Ok(())
    }
});
