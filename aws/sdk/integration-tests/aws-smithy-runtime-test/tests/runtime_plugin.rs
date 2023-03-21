/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime::BoxError;
use aws_smithy_runtime_api::config_bag::ConfigBag;
use aws_smithy_runtime_api::runtime_plugin::RuntimePlugin;

#[derive(Debug)]
struct ClientPlugin;

impl RuntimePlugin for ClientPlugin {
    fn configure(&self, _cfg: &mut ConfigBag) -> Result<(), BoxError> {
        todo!()
    }
}

#[derive(Debug)]
struct OperationPlugin;

impl RuntimePlugin for OperationPlugin {
    fn configure(&self, _cfg: &mut ConfigBag) -> Result<(), BoxError> {
        todo!()
    }
}

#[tokio::test]
async fn plugins_can_be_configured_at_various_levels() {
    // The AWS config level
    let sdk_config = aws_config::from_env()
        .runtime_plugin(ClientPlugin {})
        .load()
        .await;

    // The service config level
    let s3_config = aws_sdk_s3::config::Builder::from(&sdk_config)
        .runtime_plugin(ClientPlugin {})
        .build();

    // The operation config level
    let s3_client = aws_sdk_s3::Client::from_conf(s3_config);
    let _ = s3_client
        .create_bucket()
        .bucket("mybucket")
        .runtime_plugin(OperationPlugin {})
        .send()
        .await;
}
