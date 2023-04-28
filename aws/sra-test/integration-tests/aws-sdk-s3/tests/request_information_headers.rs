/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_http::user_agent::AwsUserAgent;
use aws_runtime::invocation_id::InvocationId;
use aws_sdk_s3::config::{Credentials, Region};
use aws_sdk_s3::endpoint::Params;
use aws_sdk_s3::Client;
use aws_smithy_client::dvr;
use aws_smithy_client::dvr::MediaType;
use aws_smithy_client::erase::DynConnector;
use aws_smithy_runtime::client::retries::strategy::FixedDelayRetryStrategy;
use aws_smithy_runtime_api::client::orchestrator::{ConfigBagAccessors, RequestTime};
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;
use aws_smithy_runtime_api::config_bag::ConfigBag;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct FixupPlugin {
    client: Client,
    timestamp: SystemTime,
}

// # One SDK operation invocation.
// # Client retries 3 times, successful response on 3rd attempt.
// # Fast network, latency + server time is less than one second.
// # No clock skew
// # Client waits 1 second between retry attempts.
#[tokio::test]
async fn three_retries_and_then_success() {
    tracing_subscriber::fmt::init();

    impl RuntimePlugin for FixupPlugin {
        fn configure(
            &self,
            cfg: &mut ConfigBag,
        ) -> Result<(), aws_smithy_runtime_api::client::runtime_plugin::BoxError> {
            let params_builder = Params::builder()
                .set_region(self.client.conf().region().map(|c| c.as_ref().to_string()))
                .bucket("test-bucket");

            cfg.put(params_builder);
            cfg.set_request_time(RequestTime::new(self.timestamp.clone()));
            cfg.put(AwsUserAgent::for_tests());
            cfg.put(InvocationId::for_tests());
            cfg.set_retry_strategy(FixedDelayRetryStrategy::one_second_delay());
            Ok(())
        }
    }

    let path = "test-data/request-information-headers/three-retries_and-then-success.json";
    let conn = dvr::ReplayingConnection::from_file(path).unwrap();
    let config = aws_sdk_s3::Config::builder()
        .credentials_provider(Credentials::for_tests())
        .region(Region::new("us-east-1"))
        .http_connector(DynConnector::new(conn.clone()))
        .build();
    let client = Client::from_conf(config);
    let fixup = FixupPlugin {
        client: client.clone(),
        timestamp: UNIX_EPOCH + Duration::from_secs(1624036048),
    };

    let resp = dbg!(
        client
            .list_objects_v2()
            .config_override(aws_sdk_s3::Config::builder().force_path_style(false))
            .bucket("test-bucket")
            .prefix("prefix~")
            .send_v2_with_plugin(Some(fixup))
            .await
    );

    let resp = resp.expect("valid e2e test");
    assert_eq!(resp.name(), Some("test-bucket"));
    conn.full_validate(MediaType::Xml).await.expect("failed")
}
//
// // # Client makes 3 separate SDK operation invocations
// // # All succeed on first attempt.
// // # Fast network, latency + server time is less than one second.
// // - request:
// //     time: 2019-06-01T00:00:00Z
// //     headers:
// //       amz-sdk-invocation-id: 3dfe4f26-c090-4887-8c14-7bac778bca07
// //       amz-sdk-request: attempt=1; max=3
// //   response:
// //     status: 200
// //     time_received: 2019-06-01T00:00:00Z
// //     headers:
// //       Date: Sat, 01 Jun 2019 00:00:00 GMT
// // - request:
// //     time: 2019-06-01T00:01:01Z
// //     headers:
// //       # Note the different invocation id because it's a new SDK
// //       # invocation operation.
// //       amz-sdk-invocation-id: 70370531-7b83-4b90-8b93-46975687ecf6
// //       amz-sdk-request: ttl=20190601T000011Z; attempt=1; max=3
// //   response:
// //     status: 200
// //     time_received: 2019-06-01T00:00:01Z
// //     headers:
// //       Date: Sat, 01 Jun 2019 00:00:01 GMT
// // - request:
// //     time: 2019-06-01T00:00:02Z
// //     headers:
// //       amz-sdk-invocation-id: 910bf450-6c90-43de-a508-3fa126a06b71
// //       amz-sdk-request: ttl=20190601T000012Z; attempt=1; max=3
// //   response:
// //     status: 200
// //     time_received: 2019-06-01T00:00:02Z
// //     headers:
// //       Date: Sat, 01 Jun 2019 00:00:02 GMT
// const THREE_SUCCESSFUL_ATTEMPTS_PATH: &str = "test-data/request-information-headers/three-successful-attempts.json";
// #[tokio::test]
// async fn three_successful_attempts() {
//     tracing_subscriber::fmt::init();
//
//     impl RuntimePlugin for FixupPlugin {
//         fn configure(
//             &self,
//             cfg: &mut ConfigBag,
//         ) -> Result<(), aws_smithy_runtime_api::client::runtime_plugin::BoxError> {
//             let params_builder = Params::builder()
//                 .set_region(self.client.conf().region().map(|c| c.as_ref().to_string()))
//                 .bucket("test-bucket");
//
//             cfg.put(params_builder);
//             cfg.set_request_time(RequestTime::new(self.timestamp.clone()));
//             cfg.put(AwsUserAgent::for_tests());
//             cfg.put(InvocationId::for_tests());
//             Ok(())
//         }
//     }
//
//     let conn = dvr::ReplayingConnection::from_file(THREE_SUCCESSFUL_ATTEMPTS_PATH).unwrap();
//     let config = aws_sdk_s3::Config::builder()
//         .credentials_provider(Credentials::for_tests())
//         .region(Region::new("us-east-1"))
//         .http_connector(DynConnector::new(conn.clone()))
//         .build();
//     let client = Client::from_conf(config);
//     let fixup = FixupPlugin {
//         client: client.clone(),
//         timestamp: UNIX_EPOCH + Duration::from_secs(1624036048),
//     };
//
//     let resp = dbg!(
//         client
//             .list_objects_v2()
//             .config_override(aws_sdk_s3::Config::builder().force_path_style(false))
//             .bucket("test-bucket")
//             .prefix("prefix~")
//             .send_v2_with_plugin(Some(fixup))
//             .await
//     );
//
//     let resp = resp.expect("valid e2e test");
//     assert_eq!(resp.name(), Some("test-bucket"));
//     conn.full_validate(MediaType::Xml).await.expect("failed")
// }
//
// // # One SDK operation invocation.
// // # Client retries 3 times, successful response on 3rd attempt.
// // # Slow network, one way latency is 2 seconds.
// // # Server takes 1 second to generate response.
// // # Client clock is 10 minutes behind server clock.
// // # One second delay between retries.
// // - request:
// //     time: 2019-06-01T00:00:00Z
// //     headers:
// //       amz-sdk-invocation-id: 3dfe4f26-c090-4887-8c14-7bac778bca07
// //       amz-sdk-request: attempt=1; max=3
// //   response:
// //     status: 500
// //     time_received: 2019-06-01T00:00:05Z
// //     headers:
// //       Date: Sat, 01 Jun 2019 00:10:03 GMT
// // - request:
// //     time: 2019-06-01T00:00:06Z
// //     # The ttl is 00:00:16 with the client clock,
// //     # but accounting for skew we have
// //     # 00:10:03 - 00:00:05 = 00:09:58
// //     # ttl = 00:00:16 + 00:09:58 = 00:10:14
// //     headers:
// //       amz-sdk-invocation-id: 3dfe4f26-c090-4887-8c14-7bac778bca07
// //       amz-sdk-request: ttl=20190601T001014Z; attempt=2; max=3
// //   response:
// //     status: 500
// //     time_received: 2019-06-01T00:00:11Z
// //     headers:
// //       Date: Sat, 01 Jun 2019 00:10:09 GMT
// // - request:
// //     time: 2019-06-01T00:00:12Z
// //     headers:
// //       # ttl = 00:00:12 + 20 = 00:00:22
// //       # skew is:
// //       # 00:10:09 - 00:00:11
// //       amz-sdk-invocation-id: 3dfe4f26-c090-4887-8c14-7bac778bca07
// //       amz-sdk-request: ttl=20190601T001020Z; attempt=3; max=3
// //   response:
// //     status: 200
// //     time_received: 2019-06-01T00:00:17Z
// //     headers:
// //       Date: Sat, 01 Jun 2019 00:10:15 GMT
// const SLOW_NETWORK_AND_LATE_CLIENT_CLOCK_PATH: &str = "test-data/request-information-headers/slow-network-and-late-client-clock.json";
// #[tokio::test]
// async fn slow_network_and_late_client_clock() {
//     tracing_subscriber::fmt::init();
//
//     impl RuntimePlugin for FixupPlugin {
//         fn configure(
//             &self,
//             cfg: &mut ConfigBag,
//         ) -> Result<(), aws_smithy_runtime_api::client::runtime_plugin::BoxError> {
//             let params_builder = Params::builder()
//                 .set_region(self.client.conf().region().map(|c| c.as_ref().to_string()))
//                 .bucket("test-bucket");
//
//             cfg.put(params_builder);
//             cfg.set_request_time(RequestTime::new(self.timestamp.clone()));
//             cfg.put(AwsUserAgent::for_tests());
//             cfg.put(InvocationId::for_tests());
//             Ok(())
//         }
//     }
//
//     let conn = dvr::ReplayingConnection::from_file(SLOW_NETWORK_AND_LATE_CLIENT_CLOCK_PATH).unwrap();
//     let config = aws_sdk_s3::Config::builder()
//         .credentials_provider(Credentials::for_tests())
//         .region(Region::new("us-east-1"))
//         .http_connector(DynConnector::new(conn.clone()))
//         .build();
//     let client = Client::from_conf(config);
//     let fixup = FixupPlugin {
//         client: client.clone(),
//         timestamp: UNIX_EPOCH + Duration::from_secs(1624036048),
//     };
//
//     let resp = dbg!(
//         client
//             .list_objects_v2()
//             .config_override(aws_sdk_s3::Config::builder().force_path_style(false))
//             .bucket("test-bucket")
//             .prefix("prefix~")
//             .send_v2_with_plugin(Some(fixup))
//             .await
//     );
//
//     let resp = resp.expect("valid e2e test");
//     assert_eq!(resp.name(), Some("test-bucket"));
//     conn.full_validate(MediaType::Xml).await.expect("failed")
// }
