/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::{mk_canary, CanaryEnv};
use aws_config::SdkConfig;
use aws_sdk_s3 as s3;

mk_canary!("s3", |sdk_config: &SdkConfig, env: &CanaryEnv| {
    let client = s3::Client::new(sdk_config);
    let env = env.clone();
    async move {
        let _ = client
            .list_objects_v2()
            .bucket(env.s3_bucket_name.clone())
            .send()
            .await;
        Ok(())
    }
});
