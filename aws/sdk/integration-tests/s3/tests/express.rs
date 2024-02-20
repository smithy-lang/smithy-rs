/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::time::{Duration, SystemTime};

use aws_config::Region;
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::primitives::SdkBody;
use aws_sdk_s3::{Client, Config};
use aws_smithy_runtime::client::http::test_util::dvr::ReplayingClient;
use aws_smithy_runtime::client::http::test_util::{
    capture_request, ReplayEvent, StaticReplayClient,
};
use aws_smithy_runtime::test_util::capture_test_logs::capture_test_logs;
use http::Uri;

// TODO(S3Express): Convert this test to the S3 express section in canary
#[tokio::test]
async fn list_objects_v2() {
    let _logs = capture_test_logs();

    let http_client =
        ReplayingClient::from_file("tests/data/express/list-objects-v2.json").unwrap();
    let config = aws_config::from_env()
        .http_client(http_client.clone())
        .no_credentials()
        .region("us-west-2")
        .load()
        .await;
    let config = Config::from(&config)
        .to_builder()
        .with_test_defaults()
        .build();
    let client = aws_sdk_s3::Client::from_conf(config);

    let result = client
        .list_objects_v2()
        .bucket("s3express-test-bucket--usw2-az1--x-s3")
        .send()
        .await;
    dbg!(result).expect("success");

    http_client
        .validate_body_and_headers(Some(&["x-amz-s3session-token"]), "application/xml")
        .await
        .unwrap();
}

#[tokio::test]
async fn mixed_auths() {
    let _logs = capture_test_logs();

    let http_client = ReplayingClient::from_file("tests/data/express/mixed-auths.json").unwrap();
    let config = aws_config::from_env()
        .http_client(http_client.clone())
        .no_credentials()
        .region("us-west-2")
        .load()
        .await;
    let config = Config::from(&config)
        .to_builder()
        .with_test_defaults()
        .build();
    let client = aws_sdk_s3::Client::from_conf(config);

    // A call to an S3 Express bucket where we should see two request/response pairs,
    // one for the `create_session` API and the other for `list_objects_v2` in S3 Express bucket.
    let result = client
        .list_objects_v2()
        .bucket("s3express-test-bucket--usw2-az1--x-s3")
        .send()
        .await;
    dbg!(result).expect("success");

    // A call to a regular bucket, and request headers should not contain `x-amz-s3session-token`.
    let result = client
        .list_objects_v2()
        .bucket("regular-test-bucket")
        .send()
        .await;
    dbg!(result).expect("success");

    // A call to another S3 Express bucket where we should again see two request/response pairs,
    // one for the `create_session` API and the other for `list_objects_v2` in S3 Express bucket.
    let result = client
        .list_objects_v2()
        .bucket("s3express-test-bucket-2--usw2-az3--x-s3")
        .send()
        .await;
    dbg!(result).expect("success");

    // This call should be an identity cache hit for the first S3 Express bucket,
    // thus no HTTP request should be sent to the `create_session` API.
    let result = client
        .list_objects_v2()
        .bucket("s3express-test-bucket--usw2-az1--x-s3")
        .send()
        .await;
    dbg!(result).expect("success");

    http_client
        .validate_body_and_headers(Some(&["x-amz-s3session-token"]), "application/xml")
        .await
        .unwrap();
}

fn create_session_request() -> http::Request<SdkBody> {
    http::Request::builder()
        .uri("https://s3express-test-bucket--usw2-az1--x-s3.s3express-usw2-az1.us-west-2.amazonaws.com/?session")
        .header("x-amz-create-session-mode", "ReadWrite")
        .method("GET")
        .body(SdkBody::empty())
        .unwrap()
}

fn create_session_response() -> http::Response<SdkBody> {
    http::Response::builder()
        .status(200)
        .body(SdkBody::from(
            r#"<?xml version="1.0" encoding="UTF-8"?>
            <CreateSessionResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
                <Credentials>
                    <SessionToken>TESTSESSIONTOKEN</SessionToken>
                    <SecretAccessKey>TESTSECRETKEY</SecretAccessKey>
                    <AccessKeyId>ASIARTESTID</AccessKeyId>
                    <Expiration>2024-01-29T18:53:01Z</Expiration>
                </Credentials>
            </CreateSessionResult>
            "#,
        ))
        .unwrap()
}

#[tokio::test]
async fn presigning() {
    let http_client = StaticReplayClient::new(vec![ReplayEvent::new(
        create_session_request(),
        create_session_response(),
    )]);

    let config = aws_sdk_s3::Config::builder()
        .http_client(http_client)
        .region(Region::new("us-west-2"))
        .with_test_defaults()
        .build();
    let client = Client::from_conf(config);

    let presigning_config = PresigningConfig::builder()
        .start_time(SystemTime::UNIX_EPOCH + Duration::from_secs(1234567891))
        .expires_in(Duration::from_secs(30))
        .build()
        .unwrap();

    let presigned = client
        .get_object()
        .bucket("s3express-test-bucket--usw2-az1--x-s3")
        .key("ferris.png")
        .presigned(presigning_config)
        .await
        .unwrap();

    let uri = presigned.uri().parse::<Uri>().unwrap();

    let pq = uri.path_and_query().unwrap();
    let path = pq.path();
    let query = pq.query().unwrap();
    let mut query_params: Vec<&str> = query.split('&').collect();
    query_params.sort();

    pretty_assertions::assert_eq!(
        "s3express-test-bucket--usw2-az1--x-s3.s3express-usw2-az1.us-west-2.amazonaws.com",
        uri.authority().unwrap()
    );
    assert_eq!("GET", presigned.method());
    assert_eq!("/ferris.png", path);
    pretty_assertions::assert_eq!(
        &[
            "X-Amz-Algorithm=AWS4-HMAC-SHA256",
            "X-Amz-Credential=ASIARTESTID%2F20090213%2Fus-west-2%2Fs3express%2Faws4_request",
            "X-Amz-Date=20090213T233131Z",
            "X-Amz-Expires=30",
            "X-Amz-S3session-Token=TESTSESSIONTOKEN",
            "X-Amz-Signature=c09c93c7878184492cb960d59e148af932dff6b19609e63e3484599903d97e44",
            "X-Amz-SignedHeaders=host",
            "x-id=GetObject"
        ][..],
        &query_params
    );
    assert_eq!(presigned.headers().count(), 0);
}

#[tokio::test]
async fn disable_s3_express_session_auth_at_service_client_level() {
    let (http_client, request) = capture_request(None);
    let conf = Config::builder()
        .http_client(http_client)
        .region(Region::new("us-west-2"))
        .with_test_defaults()
        .disable_s3_express_session_auth(true)
        .build();
    let client = Client::from_conf(conf);

    let _ = client
        .list_objects_v2()
        .bucket("s3express-test-bucket--usw2-az1--x-s3")
        .send()
        .await;

    let req = request.expect_request();
    assert!(
        !req.headers()
            .get("authorization")
            .unwrap()
            .contains("x-amz-create-session-mode"),
        "x-amz-create-session-mode should not appear in headers when S3 Express session auth is disabled"
    );
}

#[tokio::test]
async fn disable_s3_express_session_auth_at_operation_level() {
    let (http_client, request) = capture_request(None);
    let conf = Config::builder()
        .http_client(http_client)
        .region(Region::new("us-west-2"))
        .with_test_defaults()
        .build();
    let client = Client::from_conf(conf);

    let _ = client
        .list_objects_v2()
        .bucket("s3express-test-bucket--usw2-az1--x-s3")
        .customize()
        .config_override(Config::builder().disable_s3_express_session_auth(true))
        .send()
        .await;

    let req = request.expect_request();
    assert!(
        !req.headers()
            .get("authorization")
            .unwrap()
            .contains("x-amz-create-session-mode"),
        "x-amz-create-session-mode should not appear in headers when S3 Express session auth is disabled"
    );
}
