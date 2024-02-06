/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_config::timeout::TimeoutConfig;
use aws_config::BehaviorVersion;
use aws_sdk_dynamodb::config::retry::RetryConfig;
use aws_sdk_dynamodb::types::AttributeValue;
use aws_smithy_async::test_util::instant_time_and_sleep;
use std::time::SystemTime;

#[tokio::main]
async fn main() {
    let conf = aws_config::defaults(BehaviorVersion::latest())
        .test_credentials()
        .endpoint_url("http://localhost:3000")
        .retry_config(RetryConfig::standard()) //.with_max_attempts(3))
        //.sleep_impl(instant_time_and_sleep(SystemTime::now()).1)
        //.timeout_config(TimeoutConfig::disabled())
        .load()
        .await;
    let client = aws_sdk_s3::Client::from_conf(
        aws_sdk_s3::config::Builder::from(&conf)
            .force_path_style(true)
            .build(),
    );
    let mut id = 0;
    loop {
        let err = client
            .get_object()
            .bucket("foo")
            .key(format!("bar{id}"))
            .send()
            .await;
        dbg!(err);
        println!("request finish. starting request {}", id);
        id += 1;
    }
}
