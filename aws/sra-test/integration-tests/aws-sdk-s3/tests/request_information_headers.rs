/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_http::user_agent::AwsUserAgent;
use aws_runtime::invocation_id::{
    InvocationId, InvocationIdGenerator, InvocationIdGeneratorForTests,
};
use aws_sdk_s3::config::timeout::TimeoutConfig;
use aws_sdk_s3::config::Credentials;
use aws_sdk_s3::endpoint::Params;
use aws_sdk_s3::primitives::SdkBody;
use aws_sdk_s3::Client;
use aws_smithy_async::rt::sleep::AsyncSleep;
use aws_smithy_async::test_util::{instant_time_and_sleep, ManualTimeSource};
use aws_smithy_client::http_connector::HttpConnector;
use aws_smithy_runtime::client::connections::test_connection::{ConnectionEvent, TestConnection};
use aws_smithy_runtime::client::retries::strategy::FixedDelayRetryStrategy;
use aws_smithy_runtime_api::client::interceptors::InterceptorRegistrar;
use aws_smithy_runtime_api::client::orchestrator::{ConfigBagAccessors, HttpRequest, HttpResponse};
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;
use aws_smithy_runtime_api::config_bag::ConfigBag;
use aws_smithy_types::retry::RetryConfig;
use aws_types::region::Region;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

#[derive(Debug)]
struct FixupPlugin {
    client: Client,
    connection: TestConnection,
    time_source: ManualTimeSource,
    invocation_ids: Vec<InvocationId>,
}

impl RuntimePlugin for FixupPlugin {
    fn configure(
        &self,
        cfg: &mut ConfigBag,
        _interceptors: &mut InterceptorRegistrar,
    ) -> Result<(), aws_smithy_runtime_api::client::runtime_plugin::BoxError> {
        let params_builder = Params::builder()
            .set_region(self.client.conf().region().map(|c| c.as_ref().to_string()))
            .bucket("test-bucket");

        // When invocation IDs are set, insert them into the invocation id generator for tests so we
        // can play them back.
        if !self.invocation_ids.is_empty() {
            let gen = InvocationIdGeneratorForTests::new(self.invocation_ids.clone());
            cfg.put::<Box<dyn InvocationIdGenerator>>(Box::new(gen));
        }

        cfg.set_connection(self.connection.clone());
        cfg.put(params_builder);
        cfg.set_time_source(self.time_source.clone());
        cfg.put(AwsUserAgent::for_tests());
        cfg.set_retry_strategy(FixedDelayRetryStrategy::one_second_delay());
        Ok(())
    }
}

fn create_test_req(
    amz_sdk_invocation_id: &'static str,
    amz_sdk_request: &'static str,
    x_amz_date: &'static str,
    authorization: &'static str,
) -> HttpRequest {
    http::Request::builder()
        .header(
            "x-amz-user-agent",
            "aws-sdk-rust/0.123.test api/test-service/0.123 os/windows/XPSP3 lang/rust/1.50.0",
        )
        .header(
            "x-amz-content-sha256",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        )
        .header("x-amz-security-token", "notarealsessiontoken")
        .header("amz-sdk-invocation-id", amz_sdk_invocation_id)
        .header("amz-sdk-request", amz_sdk_request)
        .header("x-amz-date", x_amz_date)
        .header("authorization", authorization)
        .uri(http::Uri::from_static(
            "https://test-bucket.s3.us-east-1.amazonaws.com/?list-type=2",
        ))
        .body(SdkBody::empty())
        .unwrap()
}

fn create_500_res(date: &'static str) -> HttpResponse {
    http::Response::builder()
        .header(
            "x-amz-id-2",
            "rbipIUyF3YKPIcqpz6hrP9x9mzYMSqkHzDEp6TEN/STcKvylDIE/LLN6x9t6EKJRrgctNsdNHWk=",
        )
        .header("x-amz-request-id", "K8036R3D4NZNMMVC")
        .header("date", date)
        .header("server", "AmazonS3")
        .header("content-length", "224")
        .header("content-type", "application/xml")
        .header("transfer-encoding", "chunked")
        .status(500)
        .body(SdkBody::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Error>\n    <Type>Server</Type>\n    <Code>InternalError</Code>\n    <Message>We encountered an internal error. Please try again.</Message>\n    <RequestId>foo-id</RequestId>\n</Error>\""))
        .unwrap()
}

fn create_200_res(date: &'static str) -> HttpResponse {
    http::Response::builder()
        .header(
            "x-amz-id-2",
            "rbipIUyF3YKPIcqpz6hrP9x9mzYMSqkHzDEp6TEN/STcKvylDIE/LLN6x9t6EKJRrgctNsdNHWk=",
        )
        .header("x-amz-request-id", "K8036R3D4NZNMMVC")
        .header("x-amz-bucket-region", "us-east-1")
        .header("date", date)
        .header("server", "AmazonS3")
        .header("content-type", "application/xml")
        .header("transfer-encoding", "chunked")
        .status(200)
        .body(SdkBody::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ListBucketResult>\n    <Name>test-bucket</Name>\n    <Prefix>prefix~</Prefix>\n    <KeyCount>1</KeyCount>\n    <MaxKeys>1000</MaxKeys>\n    <IsTruncated>false</IsTruncated>\n    <Contents>\n        <Key>some-file.file</Key>\n        <LastModified>2009-10-12T17:50:30.000Z</LastModified>\n        <Size>434234</Size>\n        <StorageClass>STANDARD</StorageClass>\n    </Contents>\n</ListBucketResult>"))
        .unwrap()
}

// # One SDK operation invocation.
// # Client retries 3 times, successful response on 3rd attempt.
// # Fast network, latency + server time is less than one second.
// # No clock skew
// # Client waits 1 second between retry attempts.
#[tokio::test]
async fn three_retries_and_then_success() {
    let timeout_config = TimeoutConfig::builder()
        .read_timeout(Duration::from_secs(10))
        .build();

    let events = [
        (
            (
                create_test_req(
                    "3dfe4f26-c090-4887-8c14-7bac778bca07",
                    "attempt=1; max=4",
                    "20190601T000000Z",
                    "AWS4-HMAC-SHA256 Credential=ANOTREAL/20190601/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token;x-amz-user-agent, Signature=cf35e4ae3106226994f83b030d76bd80c61600199fd51dcb448b3587c29a9754"
                ),
                create_500_res("Sat, 01 Jun 2019 00:00:00 GMT")
            )
        ),
        (
            (
                create_test_req(
                    "3dfe4f26-c090-4887-8c14-7bac778bca07",
                    "ttl=20190601T000011Z; attempt=2; max=4",
                    "20190601T000001Z",
                    "AWS4-HMAC-SHA256 Credential=ANOTREAL/20190601/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token;x-amz-user-agent, Signature=bb18fc779bca869d3cee506dedc065b920704be35b25df3139877f12e8149276"
                ),
                create_500_res("Sat, 01 Jun 2019 00:00:01 GMT")
            )
        ),
        (
            (
                create_test_req(
                    "3dfe4f26-c090-4887-8c14-7bac778bca07",
                    "ttl=20190601T000012Z; attempt=3; max=4",
                    "20190601T000002Z",
                    "AWS4-HMAC-SHA256 Credential=ANOTREAL/20190601/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token;x-amz-user-agent, Signature=82fd5529bc21d484eba6684d0b56881c49928b39532cda6bc81dc00cef9adac7"
                ),
                create_500_res("Sat, 01 Jun 2019 00:00:02 GMT")
            )
        ),
        (
            (
                create_test_req(
                 "3dfe4f26-c090-4887-8c14-7bac778bca07",
                 "ttl=20190601T000013Z; attempt=4; max=4",
                 "20190601T000003Z",
                 "AWS4-HMAC-SHA256 Credential=ANOTREAL/20190601/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token;x-amz-user-agent, Signature=e06b6d1cb43e4263aba325852d45fc4528645b2a5a48be20f3924d0bd0b2ffae"
                ),
                create_200_res("Sat, 01 Jun 2019 00:00:03 GMT")
            )
        ),
    ]
        .into_iter()
        .map(Into::into)
        .collect();

    let (time_source, sleep) = instant_time_and_sleep(UNIX_EPOCH + Duration::from_secs(1559347200));
    let sleep_impl: Arc<dyn AsyncSleep> = Arc::new(sleep.clone());
    let conn = TestConnection::new(events, sleep_impl.clone());
    let config = aws_sdk_s3::Config::builder()
        .credentials_provider(Credentials::for_tests())
        .http_connector(HttpConnector::Prebuilt(None))
        .retry_config(RetryConfig::standard().with_max_attempts(4))
        .timeout_config(timeout_config)
        .sleep_impl(sleep_impl.clone())
        .region(Region::new("us-east-1"))
        .build();
    let client = Client::from_conf(config);
    let fixup = FixupPlugin {
        client: client.clone(),
        connection: conn.clone(),
        time_source,
        invocation_ids: vec![InvocationId::from_str(
            "3dfe4f26-c090-4887-8c14-7bac778bca07",
        )],
    };

    let res = client
        .list_objects_v2()
        .bucket("test-bucket")
        .send_orchestrator_with_plugin(Some(fixup))
        .await
        .expect("valid e2e test");

    // Assert that we slept three times, for 1 sec each.
    assert_eq!(
        vec![
            Duration::from_secs(1),
            Duration::from_secs(1),
            Duration::from_secs(1)
        ],
        sleep.logs()
    );

    assert_eq!(res.name(), Some("test-bucket"));
    conn.assert_requests_match(&[]);
}

// # Client makes 3 separate SDK operation invocations
// # All succeed on first attempt.
// # Fast network, latency + server time is less than one second.
// # No clock skew
// # Client waits 1 second between attempts.
// TODO(orchestratorCrossOperationState) We don't support cross-operation state yet, so we can't calculate the service time skew for operations being sent after the first one.
#[ignore = "We don't support cross-operation state yet, so we can't calculate the skew for operations being sent after the first one."]
#[tokio::test]
async fn three_successful_attempts() {
    let timeout_config = TimeoutConfig::builder()
        .read_timeout(Duration::from_secs(10))
        .build();

    let events = [
        (
            (create_test_req(
                 "3dfe4f26-c090-4887-8c14-7bac778bca07",
                 "attempt=1; max=3",
                 "20190601T000000Z",
                 "AWS4-HMAC-SHA256 Credential=ANOTREAL/20190601/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token;x-amz-user-agent, Signature=cf35e4ae3106226994f83b030d76bd80c61600199fd51dcb448b3587c29a9754"
             ),
             create_200_res("Sat, 01 Jun 2019 00:00:00 GMT"))

        ),
        (
            (
                create_test_req(
                 "70370531-7b83-4b90-8b93-46975687ecf6",
                 "ttl=20190601T000011Z; attempt=1; max=3",
                 "20190601T000001Z",
                 "AWS4-HMAC-SHA256 Credential=ANOTREAL/20190601/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token;x-amz-user-agent, Signature=bb18fc779bca869d3cee506dedc065b920704be35b25df3139877f12e8149276"
             ),
             create_200_res("Sat, 01 Jun 2019 00:00:01 GMT"))
        ),
        (
            (
             create_test_req(
                 "910bf450-6c90-43de-a508-3fa126a06b71",
                 "ttl=20190601T000012Z; attempt=1; max=3",
                 "20190601T000002Z",
                 "AWS4-HMAC-SHA256 Credential=ANOTREAL/20190601/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token;x-amz-user-agent, Signature=82fd5529bc21d484eba6684d0b56881c49928b39532cda6bc81dc00cef9adac7"
             ),
             create_200_res("Sat, 01 Jun 2019 00:00:02 GMT"))
        ),
    ]
        .into_iter()
        .map(Into::into)
        .collect();

    let (time_source, sleep) = instant_time_and_sleep(UNIX_EPOCH + Duration::from_secs(1559347200));
    let sleep_impl: Arc<dyn AsyncSleep> = Arc::new(sleep.clone());
    let conn = TestConnection::new(events, sleep_impl.clone());
    let config = aws_sdk_s3::Config::builder()
        .credentials_provider(Credentials::for_tests())
        .http_connector(HttpConnector::Prebuilt(None))
        .retry_config(RetryConfig::standard().with_max_attempts(3))
        .timeout_config(timeout_config)
        .sleep_impl(Arc::new(sleep_impl.clone()))
        .region(Region::new("us-east-1"))
        .build();

    let client = Client::from_conf(config);

    for id in [
        InvocationId::from_str("3dfe4f26-c090-4887-8c14-7bac778bca07"),
        InvocationId::from_str("70370531-7b83-4b90-8b93-46975687ecf6"),
        InvocationId::from_str("910bf450-6c90-43de-a508-3fa126a06b71"),
    ] {
        let fixup = FixupPlugin {
            client: client.clone(),
            connection: conn.clone(),
            time_source: time_source.clone(),
            invocation_ids: vec![id],
        };

        let res = client
            .list_objects_v2()
            .bucket("test-bucket")
            .send_orchestrator_with_plugin(Some(fixup))
            .await
            .expect("valid e2e test");

        // Assert that we never slept because we never retried a request
        assert!(sleep.logs().is_empty());
        assert_eq!(res.name(), Some("test-bucket"));
    }

    conn.assert_requests_match(&[]);
}

// # One SDK operation invocation.
// # Client retries 3 times, successful response on 3rd attempt.
// # Slow network, total latency is 5 seconds:
// #     One way latency is 2 seconds.
// #     Server takes 1 second to generate response.
// # Client clock is 10 minutes behind server clock.
// # One second delay between retries.
#[tokio::test]
async fn slow_network_and_late_client_clock() {
    tracing_subscriber::fmt::init();

    let timeout_config = TimeoutConfig::builder()
        .read_timeout(Duration::from_secs(10))
        .build();

    let events = [
        ConnectionEvent::new(
              create_test_req(
                  "3dfe4f26-c090-4887-8c14-7bac778bca07",
                  "attempt=1; max=3",
                  "20190601T000000Z",
                  "AWS4-HMAC-SHA256 Credential=ANOTREAL/20190601/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token;x-amz-user-agent, Signature=cf35e4ae3106226994f83b030d76bd80c61600199fd51dcb448b3587c29a9754"
              ),
              create_500_res("Sat, 01 Jun 2019 00:10:03 GMT")
        ).with_latency(Duration::from_secs(5)),
        ConnectionEvent::new(
            create_test_req(
                "3dfe4f26-c090-4887-8c14-7bac778bca07",
                "ttl=20190601T001014Z; attempt=2; max=3",
                "20190601T000006Z",
                "AWS4-HMAC-SHA256 Credential=ANOTREAL/20190601/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token;x-amz-user-agent, Signature=4d496616bd3867686426f6eabf36a338959ce247f5839688e61ea0a7e6bbe9f7"
            ),
             create_500_res("Sat, 01 Jun 2019 00:10:09 GMT")
        ).with_latency(Duration::from_secs(5)),
        ConnectionEvent::new(
            create_test_req(
                "3dfe4f26-c090-4887-8c14-7bac778bca07",
                "ttl=20190601T001020Z; attempt=3; max=3",
                "20190601T000012Z",
                "AWS4-HMAC-SHA256 Credential=ANOTREAL/20190601/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token;x-amz-user-agent, Signature=19a55486340057d1f5a9318cbfc9cbde0ead9d42df7f9a5ca98fc96b46235224"
            ),
            create_200_res("Sat, 01 Jun 2019 00:10:15 GMT")
        ),
    ].into_iter().collect();

    let (time_source, sleep) = instant_time_and_sleep(UNIX_EPOCH + Duration::from_secs(1559347200));
    let sleep_impl: Arc<dyn AsyncSleep> = Arc::new(sleep);
    let conn = TestConnection::new(events, sleep_impl.clone());
    let config = aws_sdk_s3::Config::builder()
        .credentials_provider(Credentials::for_tests())
        .http_connector(HttpConnector::Prebuilt(None))
        .retry_config(RetryConfig::standard().with_max_attempts(3))
        .timeout_config(timeout_config)
        .sleep_impl(sleep_impl)
        .region(Region::new("us-east-1"))
        .build();

    let client = Client::from_conf(config);

    let fixup = FixupPlugin {
        client: client.clone(),
        connection: conn.clone(),
        time_source,
        invocation_ids: vec![InvocationId::from_str(
            "3dfe4f26-c090-4887-8c14-7bac778bca07",
        )],
    };

    let res = client
        .list_objects_v2()
        .bucket("test-bucket")
        .send_orchestrator_with_plugin(Some(fixup))
        .await
        .expect("valid e2e test");

    assert_eq!(res.name(), Some("test-bucket"));
    conn.assert_requests_match(&[]);
}
