/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

pub mod common;

#[tokio::test]
async fn test_check_health_http2() {
    let _child = common::run_server().await;
    let client = common::client_http2_only();

    let _check_health = client.check_health().send().await.unwrap();
}
