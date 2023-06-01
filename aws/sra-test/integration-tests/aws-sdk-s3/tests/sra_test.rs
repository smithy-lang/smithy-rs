/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_s3::config::{Credentials, Region};
use aws_sdk_s3::Client;
use aws_smithy_client::dvr;
use aws_smithy_client::dvr::MediaType;
use aws_smithy_client::erase::DynConnector;
use std::time::{Duration, UNIX_EPOCH};

const LIST_BUCKETS_PATH: &str = "test-data/list-objects-v2.json";

#[tokio::test]
async fn sra_test() {
    tracing_subscriber::fmt::init();

    let conn = dvr::ReplayingConnection::from_file(LIST_BUCKETS_PATH).unwrap();

    let config = aws_sdk_s3::Config::builder()
        .credentials_provider(Credentials::for_tests())
        .region(Region::new("us-east-1"))
        .http_connector(DynConnector::new(conn.clone()))
        .interceptor(util::TestUserAgentInterceptor)
        .build();
    let client = Client::from_conf(config);
    let fixup = util::FixupPlugin;

    let resp = dbg!(
        client
            .list_objects_v2()
            .bucket("test-bucket")
            .prefix("prefix~")
            .send_orchestrator_with_plugin(Some(fixup))
            .await
    );
    // To regenerate the test:
    // conn.dump_to_file("test-data/list-objects-v2.json").unwrap();
    let resp = resp.expect("valid e2e test");
    assert_eq!(resp.name(), Some("test-bucket"));
    conn.full_validate(MediaType::Xml).await.expect("success")
}
