/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_dynamodb::config::{BehaviorVersion, SharedAsyncSleep, StalledStreamProtectionConfig};
use aws_sdk_dynamodb::{Client, Config};
use aws_smithy_async::test_util::instant_time_and_sleep;
use aws_smithy_async::time::SharedTimeSource;
use aws_smithy_http_client::test_util::{ReplayEvent, StaticReplayClient};
use aws_smithy_runtime::client::retries::RetryPartition;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::BeforeTransmitInterceptorContextMut;
use aws_smithy_runtime_api::client::interceptors::Intercept;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_types::body::SdkBody;
use aws_smithy_types::config_bag::ConfigBag;
use aws_smithy_types::retry::RetryConfig;
use std::time::{Duration, SystemTime};

fn req() -> http_1x::Request<SdkBody> {
    http_1x::Request::builder()
        .body(SdkBody::from("request body"))
        .unwrap()
}

fn ok() -> http_1x::Response<SdkBody> {
    let body = "{ \"TableNames\": [ \"Test\" ] }";
    http_1x::Response::builder()
        .status(200)
        .header("content-type", "application/x-amz-json-1.0")
        .header("content-length", body.len().to_string())
        .header("x-amz-crc32", "2335643545")
        .body(SdkBody::from(body))
        .unwrap()
}

fn err() -> http_1x::Response<SdkBody> {
    http_1x::Response::builder()
        .status(500)
        .body(SdkBody::from(
            "{ \"message\": \"Internal error\", \"code\": \"InternalServerError\" }",
        ))
        .unwrap()
}

/// Interceptor that sets static exponential base on the resolved RetryConfig
/// without overriding other fields like RetrySpec or max_attempts.
#[derive(Debug)]
struct StaticBackoffInterceptor;

impl Intercept for StaticBackoffInterceptor {
    fn name(&self) -> &'static str {
        "StaticBackoffInterceptor"
    }

    fn modify_before_retry_loop(
        &self,
        _context: &mut BeforeTransmitInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        if let Some(rc) = cfg.load::<RetryConfig>().cloned() {
            cfg.interceptor_state()
                .store_put(rc.with_use_static_exponential_base(true));
        }
        Ok(())
    }
}

/// Latest BV: DynamoDB uses 25ms backoff and 4 max attempts.
#[tokio::test]
async fn dynamodb_v2_1_uses_25ms_backoff_and_4_max_attempts() {
    let (time_source, sleep_impl) = instant_time_and_sleep(SystemTime::UNIX_EPOCH);
    let http_client = StaticReplayClient::new(vec![
        ReplayEvent::new(req(), err()),
        ReplayEvent::new(req(), err()),
        ReplayEvent::new(req(), err()),
        ReplayEvent::new(req(), ok()),
    ]);

    let config = Config::builder()
        .with_test_defaults_v2()
        .stalled_stream_protection(StalledStreamProtectionConfig::disabled())
        .interceptor(StaticBackoffInterceptor)
        .time_source(SharedTimeSource::new(time_source))
        .sleep_impl(SharedAsyncSleep::new(sleep_impl.clone()))
        .retry_partition(RetryPartition::new("dynamodb_v2_1"))
        .http_client(http_client.clone())
        .build();

    let client = Client::from_conf(config);
    let res = client.list_tables().send().await.unwrap();
    assert_eq!(res.table_names(), ["Test"]);
    assert_eq!(http_client.actual_requests().count(), 4);
    // 25ms * 2^0 + 25ms * 2^1 + 25ms * 2^2 = 175ms
    assert_eq!(sleep_impl.total_duration(), Duration::from_millis(175));
}

/// Old BV: DynamoDB uses default 1s backoff and 3 max attempts.
#[tokio::test]
#[allow(deprecated)]
async fn dynamodb_v2_0_uses_1s_backoff_and_3_max_attempts() {
    let (time_source, sleep_impl) = instant_time_and_sleep(SystemTime::UNIX_EPOCH);
    let http_client = StaticReplayClient::new(vec![
        ReplayEvent::new(req(), err()),
        ReplayEvent::new(req(), err()),
        ReplayEvent::new(req(), ok()),
    ]);

    // Old BV has retries disabled by default, so we need an interceptor
    // to enable them without overriding the full RetryConfig.
    #[derive(Debug)]
    struct EnableRetriesInterceptor;
    impl Intercept for EnableRetriesInterceptor {
        fn name(&self) -> &'static str {
            "EnableRetriesInterceptor"
        }
        fn modify_before_retry_loop(
            &self,
            _context: &mut BeforeTransmitInterceptorContextMut<'_>,
            _runtime_components: &RuntimeComponents,
            cfg: &mut ConfigBag,
        ) -> Result<(), BoxError> {
            if let Some(rc) = cfg.load::<RetryConfig>().cloned() {
                cfg.interceptor_state().store_put(
                    rc.with_max_attempts(3)
                        .with_use_static_exponential_base(true),
                );
            }
            Ok(())
        }
    }

    let config = Config::builder()
        .with_test_defaults_v2()
        .behavior_version(BehaviorVersion::v2024_03_28())
        .stalled_stream_protection(StalledStreamProtectionConfig::disabled())
        .interceptor(EnableRetriesInterceptor)
        .time_source(SharedTimeSource::new(time_source))
        .sleep_impl(SharedAsyncSleep::new(sleep_impl.clone()))
        .retry_partition(RetryPartition::new("dynamodb_v2_0"))
        .http_client(http_client.clone())
        .build();

    let client = Client::from_conf(config);
    let res = client.list_tables().send().await.unwrap();
    assert_eq!(res.table_names(), ["Test"]);
    assert_eq!(http_client.actual_requests().count(), 3);
    // 1s * 2^0 + 1s * 2^1 = 3s
    assert_eq!(sleep_impl.total_duration(), Duration::from_secs(3));
}
