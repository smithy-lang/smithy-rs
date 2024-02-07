/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_config::timeout::TimeoutConfig;
use aws_config::BehaviorVersion;
use aws_sdk_dynamodb::config::retry::RetryConfig;
use aws_sdk_dynamodb::types::AttributeValue;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let conf = aws_config::defaults(BehaviorVersion::latest())
        .test_credentials()
        .endpoint_url("http://localhost:3000")
        .retry_config(
            RetryConfig::standard(), /*.with_max_backoff(Duration::from_secs(1)), */
                                     /*.with_reconnect_mode(ReconnectMode::ReuseAllConnections)*/
        )
        //.sleep_impl(instant_time_and_sleep(SystemTime::now()).1)
        .timeout_config(
            TimeoutConfig::builder()
                .operation_attempt_timeout(Duration::from_secs(1))
                .build(),
        )
        .load()
        .await;
    let client = aws_sdk_dynamodb::Client::new(&conf);
    let mut id = 0;
    loop {
        let _err = client
            .get_item()
            .key("k", AttributeValue::N(id.to_string()))
            .send()
            .await;
        println!("request finish. starting request {}", id);
        id += 1;
    }
}
