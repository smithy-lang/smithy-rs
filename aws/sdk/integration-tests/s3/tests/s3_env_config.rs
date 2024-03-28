/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#[allow(deprecated)]
use aws_config::profile::profile_file::{ProfileFileKind, ProfileFiles};
use aws_sdk_s3::config::BehaviorVersion;
use aws_sdk_s3::error::DisplayErrorContext;
use aws_sdk_s3::primitives::SdkBody;
use aws_sdk_s3::Client;
use aws_smithy_runtime::client::http::test_util::{ReplayEvent, StaticReplayClient};
use aws_smithy_runtime::test_util::capture_test_logs::capture_test_logs;
use http::Uri;

const NOT_LIKE_OTHER_SERVICE_SPECIFIC_CONFIGURATION: &str = r#"[default]
services = foo
s3_use_arn_region = false
region = us-east-1
services = foo

[services foo]
s3 =
    s3_use_arn_region = true
    region = us-west-2
"#;

#[tokio::test]
async fn s3_env_config_works() {
    let (_guard, _logs_rx) = capture_test_logs();
    let http_client= StaticReplayClient::new(vec![ReplayEvent::new(
        http::Request::builder()
            .uri(Uri::from_static("https://kms.cn-north-1.amazonaws.com.cn/"))
            .body(SdkBody::from(r#"{"NumberOfBytes":64}"#)).unwrap(),
        http::Response::builder()
            .status(http::StatusCode::from_u16(200).unwrap())
            .body(SdkBody::from(r#"{"Plaintext":"6CG0fbzzhg5G2VcFCPmJMJ8Njv3voYCgrGlp3+BZe7eDweCXgiyDH9BnkKvLmS7gQhnYDUlyES3fZVGwv5+CxA=="}"#)).unwrap())
    ]);

    let sdk_config = aws_config::defaults(BehaviorVersion::latest())
        .test_credentials()
        .http_client(http_client)
        .profile_files(
            #[allow(deprecated)]
            ProfileFiles::builder()
                .with_contents(
                    #[allow(deprecated)]
                    ProfileFileKind::Config,
                    NOT_LIKE_OTHER_SERVICE_SPECIFIC_CONFIGURATION,
                )
                .build(),
        )
        .load()
        .await;

    let client = Client::new(&sdk_config);

    let err = client
        .get_object()
        .bucket("arn:aws:s3-object-lambda:us-east-1:123412341234:accesspoint/myolap")
        .key("s3.txt")
        .send()
        .await
        .expect_err("should fail—cross region invalid arn");

    let err = DisplayErrorContext(err);
    panic!("{err:?}");

    // assert!(
    //     format!("{:?}", err).contains(
    //         "Invalid configuration: region from ARN `us-east-1` \
    // does not match client region `us-west-2` and UseArnRegion is `false`"
    //     ),
    //     "{}",
    //     err
    // );
}
