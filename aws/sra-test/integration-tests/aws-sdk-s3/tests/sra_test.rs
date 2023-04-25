/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_http::user_agent::AwsUserAgent;
use aws_runtime::invocation_id::InvocationId;
use aws_sdk_s3::config::{Credentials, Region};
use aws_sdk_s3::endpoint::Params;
use aws_sdk_s3::primitives::SdkBody;
use aws_sdk_s3::Client;

use aws_smithy_client::test_connection::capture_request;
use aws_smithy_runtime_api::client::orchestrator::{ConfigBagAccessors, RequestTime};
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;
use aws_smithy_runtime_api::config_bag::ConfigBag;
use http::header::AUTHORIZATION;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const LIST_OBJECT_RESPONSE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult>
    <Name>test-bucket</Name>
    <Prefix>prefix~</Prefix>
    <KeyCount>1</KeyCount>
    <MaxKeys>1000</MaxKeys>
    <IsTruncated>false</IsTruncated>
    <Contents>
        <Key>some-file.file</Key>
        <LastModified>2009-10-12T17:50:30.000Z</LastModified>
        <Size>434234</Size>
        <StorageClass>STANDARD</StorageClass>
    </Contents>
</ListBucketResult>
"#;

#[tokio::test]
async fn sra_test() {
    tracing_subscriber::fmt::init();

    let (conn, req) = capture_request(Some(
        http::Response::builder()
            .status(200)
            .body(SdkBody::from(LIST_OBJECT_RESPONSE))
            .unwrap(),
    ));

    let config = aws_sdk_s3::Config::builder()
        .credentials_provider(Credentials::for_tests())
        .region(Region::new("us-east-1"))
        .http_connector(conn)
        .build();
    let client = aws_sdk_s3::Client::from_conf(config);
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
            .send_v2(Some(fixup))
            .await
    );
    let resp = resp.expect("valid e2e test");
    assert_eq!(resp.name(), Some("test-bucket"));

    let req = req.expect_request();
    assert_eq!(
        req.headers()
            .get(AUTHORIZATION)
            .expect("must have auth header")
            .to_str()
            .expect("valid string"),
        "AWS4-HMAC-SHA256 Credential=ANOTREAL/20210618/us-east-1/s3/aws4_request, SignedHeaders=amz-sdk-invocation-id;host;x-amz-content-sha256;x-amz-date;x-amz-security-token;x-amz-user-agent, Signature=9b2118ab1d61228408c1a3d82e9028d435b4ed8a6bc69c479c796bfc62776bfc"
    );
}

struct FixupPlugin {
    client: Client,
    timestamp: SystemTime,
}
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
        Ok(())
    }
}
