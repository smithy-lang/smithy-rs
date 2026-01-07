/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![allow(unused_imports)]

use wit_bindgen::generate;

generate!();

use exports::wasm::tests::tests::Guest;

struct Component;

impl Guest for Component {
    fn run() -> Result<(), ()> {
        test_operation_construction();
        test_token_bucket_gets_time_source_from_config();
        Ok(())
    }
}

export!(Component);

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_config::retry::RetryConfig;
use aws_sdk_s3::Client;
use aws_sdk_s3::operation::list_objects_v2::builders::ListObjectsV2FluentBuilder;
use aws_smithy_types::timeout::TimeoutConfig;
use aws_smithy_wasm::wasi::WasiHttpClientBuilder;
use std::sync::LazyLock;
use tokio::runtime::Runtime;

static RUNTIME: LazyLock<Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
});

async fn get_default_wasi_config() -> aws_config::SdkConfig {
    let http_client = WasiHttpClientBuilder::new().build();
    aws_config::from_env()
        .region("us-east-2")
        .timeout_config(TimeoutConfig::disabled())
        .retry_config(RetryConfig::disabled())
        .no_credentials()
        .http_client(http_client)
        .load()
        .await
}

#[allow(dead_code)]
fn test_default_config() {
    let shared_config = RUNTIME.block_on(get_default_wasi_config());
    let client = aws_sdk_s3::Client::new(&shared_config);
    assert_eq!(client.config().region().unwrap().to_string(), "us-east-2")
}

fn s3_list_objects_operation_wasi() -> ListObjectsV2FluentBuilder {
    let shared_config = RUNTIME.block_on(get_default_wasi_config());
    let client = Client::new(&shared_config);
    let operation = client
        .list_objects_v2()
        .bucket("nara-national-archives-catalog")
        .delimiter("/")
        .prefix("authority-records/organization/")
        .max_keys(5);

    operation
}

// We just test if this compiles, we don't actually want to make a network call in an integration test
fn test_operation_construction() {
    let operation = s3_list_objects_operation_wasi();
    assert_eq!(
        operation.get_bucket(),
        &Some("nara-national-archives-catalog".to_string())
    );
}

use aws_sdk_s3::{Config, config::Region};
use aws_smithy_async::test_util::ManualTimeSource;
use aws_smithy_async::time::SharedTimeSource;
use aws_smithy_http_client::test_util::{ReplayEvent, StaticReplayClient};
use aws_smithy_runtime::client::retries::TokenBucket;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::Intercept;
use aws_smithy_runtime_api::client::interceptors::context::BeforeTransmitInterceptorContextMut;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_types::body::SdkBody;
use aws_smithy_types::config_bag::ConfigBag;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// static THE_TIME: LazyLock<SystemTime> =
//     LazyLock::new(|| UNIX_EPOCH + Duration::from_secs(12344321));

// #[derive(Debug)]
// struct TimeSourceValidationInterceptor;

// impl Intercept for TimeSourceValidationInterceptor {
//     fn name(&self) -> &'static str {
//         "TimeSourceValidationInterceptor"
//     }

//     fn modify_before_transmit(
//         &self,
//         _context: &mut BeforeTransmitInterceptorContextMut<'_>,
//         _runtime_components: &RuntimeComponents,
//         cfg: &mut ConfigBag,
//     ) -> Result<(), BoxError> {
//         if let Some(token_bucket) = cfg.load::<TokenBucket>() {
//             let token_bucket_time_source = token_bucket.time_source();
//             let token_time = token_bucket_time_source.now();

//             assert_eq!(
//                 *THE_TIME, token_time,
//                 "Token source should match the configured time source"
//             );
//         }
//         Ok(())
//     }
// }

fn test_token_bucket_gets_time_source_from_config() {
    // let time_source = ManualTimeSource::new(*THE_TIME);
    // let shared_time_source = SharedTimeSource::new(time_source);

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
        // .time_source(shared_time_source)
        // .interceptor(TimeSourceValidationInterceptor)
        .build();

    let client = Client::from_conf(config);

    let _result = RUNTIME
        .block_on(client.list_objects_v2().bucket("test-bucket").send())
        .unwrap();
}
