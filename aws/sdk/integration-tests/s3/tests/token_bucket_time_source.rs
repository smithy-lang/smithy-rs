/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![cfg(feature = "test-util")]

use aws_sdk_s3::{config::Region, Client, Config};
use aws_smithy_async::test_util::ManualTimeSource;
use aws_smithy_async::time::SharedTimeSource;
use aws_smithy_http_client::test_util::{ReplayEvent, StaticReplayClient};
use aws_smithy_runtime::client::retries::TokenBucket;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::BeforeTransmitInterceptorContextMut;
use aws_smithy_runtime_api::client::interceptors::Intercept;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_types::body::SdkBody;
use aws_smithy_types::config_bag::ConfigBag;
use std::sync::LazyLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static THE_TIME: LazyLock<SystemTime> =
    LazyLock::new(|| UNIX_EPOCH + Duration::from_secs(12344321));

#[derive(Debug)]
struct TimeSourceValidationInterceptor;

impl Intercept for TimeSourceValidationInterceptor {
    fn name(&self) -> &'static str {
        "TimeSourceValidationInterceptor"
    }

    fn modify_before_transmit(
        &self,
        _context: &mut BeforeTransmitInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        if let Some(token_bucket) = cfg.load::<TokenBucket>() {
            let token_bucket_time_source = token_bucket.time_source();
            let token_time = token_bucket_time_source.now();

            assert_eq!(
                *THE_TIME, token_time,
                "Token source should match the configured time source"
            );
        }
        Ok(())
    }
}

#[tokio::test]
async fn test_token_bucket_gets_time_source_from_config() {
    let time_source = ManualTimeSource::new(*THE_TIME);
    let shared_time_source = SharedTimeSource::new(time_source);

    let http_client = StaticReplayClient::new(vec![ReplayEvent::new(
        http_1x::Request::builder()
            .uri("https://www.doesntmatter.com")
            .body(SdkBody::empty())
            .unwrap(),
        http_1x::Response::builder()
            .status(200)
            .body(SdkBody::from("<ListBucketResult></ListBucketResult>"))
            .unwrap(),
    )]);

    let config = Config::builder()
        .region(Region::new("us-east-1"))
        .http_client(http_client)
        .time_source(shared_time_source)
        .interceptor(TimeSourceValidationInterceptor)
        .build();

    let client = Client::from_conf(config);

    let _result = client
        .list_objects_v2()
        .bucket("test-bucket")
        .send()
        .await
        .unwrap();
}
