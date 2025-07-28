/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::time::{Duration, UNIX_EPOCH};

use aws_config::{sts::AssumeRoleProvider, BehaviorVersion, Region};
use aws_credential_types::Credentials;
use aws_runtime::user_agent::test_util::{
    assert_ua_contains_metric_values, get_sdk_metric_str_from_request,
};
use aws_sdk_s3::{config::SharedAsyncSleep, Client, Config};
use aws_smithy_async::test_util::instant_time_and_sleep;
use aws_smithy_http_client::test_util::capture_request;
use aws_smithy_runtime::client::http::test_util::{ReplayEvent, StaticReplayClient};
use aws_smithy_types::body::SdkBody;
use aws_types::SdkConfig;

#[tokio::test]
async fn sts_assume_role_ua_feature() {
    let (http_client, request) = capture_request(None);

    let creds_client = StaticReplayClient::new(vec![
            ReplayEvent::new(http::Request::new(SdkBody::from("request body")),
            http::Response::builder().status(200).body(SdkBody::from(
                "<AssumeRoleResponse xmlns=\"https://sts.amazonaws.com/doc/2011-06-15/\">\n  <AssumeRoleResult>\n    <AssumedRoleUser>\n      <AssumedRoleId>AROAR42TAWARILN3MNKUT:assume-role-from-profile-1632246085998</AssumedRoleId>\n      <Arn>arn:aws:sts::130633740322:assumed-role/assume-provider-test/assume-role-from-profile-1632246085998</Arn>\n    </AssumedRoleUser>\n    <Credentials>\n      <AccessKeyId>ASIARCORRECT</AccessKeyId>\n      <SecretAccessKey>secretkeycorrect</SecretAccessKey>\n      <SessionToken>tokencorrect</SessionToken>\n      <Expiration>2009-02-13T23:31:30Z</Expiration>\n    </Credentials>\n  </AssumeRoleResult>\n  <ResponseMetadata>\n    <RequestId>d9d47248-fd55-4686-ad7c-0fb7cd1cddd7</RequestId>\n  </ResponseMetadata>\n</AssumeRoleResponse>\n"
            )).unwrap()),
            ReplayEvent::new(http::Request::new(SdkBody::from("request body")),
            http::Response::builder().status(200).body(SdkBody::from(
                "<AssumeRoleResponse xmlns=\"https://sts.amazonaws.com/doc/2011-06-15/\">\n  <AssumeRoleResult>\n    <AssumedRoleUser>\n      <AssumedRoleId>AROAR42TAWARILN3MNKUT:assume-role-from-profile-1632246085998</AssumedRoleId>\n      <Arn>arn:aws:sts::130633740322:assumed-role/assume-provider-test/assume-role-from-profile-1632246085998</Arn>\n    </AssumedRoleUser>\n    <Credentials>\n      <AccessKeyId>ASIARCORRECT</AccessKeyId>\n      <SecretAccessKey>TESTSECRET</SecretAccessKey>\n      <SessionToken>tokencorrect</SessionToken>\n      <Expiration>2009-02-13T23:33:30Z</Expiration>\n    </Credentials>\n  </AssumeRoleResult>\n  <ResponseMetadata>\n    <RequestId>c2e971c2-702d-4124-9b1f-1670febbea18</RequestId>\n  </ResponseMetadata>\n</AssumeRoleResponse>\n"
            )).unwrap()),
        ]);

    let (testing_time_source, sleep) = instant_time_and_sleep(
        UNIX_EPOCH + Duration::from_secs(1234567890), // 1234567890 since UNIX_EPOCH is 2009-02-13T23:31:30Z from the responses above
    );

    let sts_config = SdkConfig::builder()
        .sleep_impl(SharedAsyncSleep::new(sleep))
        .time_source(testing_time_source.clone())
        .http_client(creds_client)
        .behavior_version(BehaviorVersion::latest())
        .build();

    let credentials = Credentials::new(
        "test",
        "test",
        None,
        Some(UNIX_EPOCH + Duration::from_secs(1234567890 + 1)),
        "test",
    );

    let provider = AssumeRoleProvider::builder("myrole")
        .configure(&sts_config)
        .region(Region::new("us-east-1"))
        .build_from_provider(credentials)
        .await;

    let config = Config::builder()
        .with_test_defaults()
        .region(Region::from_static("fake"))
        .http_client(http_client.clone())
        .credentials_provider(provider)
        .build();

    let client = Client::from_conf(config);

    let _ = client
        .head_bucket()
        .bucket("fake")
        .send()
        .await
        .expect("XXXXXXXXXXX");

    let request = request.expect_request();
    let ua = get_sdk_metric_str_from_request(&request);
    assert_ua_contains_metric_values(ua, &["i"]);
}
