/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_http::user_agent::AwsUserAgent;
use aws_runtime::invocation_id::InvocationId;
use aws_sdk_s3::config::{Credentials, Region};
use aws_sdk_s3::Client;
use aws_smithy_client::dvr;
use aws_smithy_client::dvr::MediaType;
use aws_smithy_client::erase::DynConnector;
use aws_smithy_runtime_api::client::interceptors::Interceptors;
use aws_smithy_runtime_api::client::orchestrator::{ConfigBagAccessors, RequestTime};
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;
use aws_smithy_runtime_api::config_bag::ConfigBag;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const LIST_BUCKETS_PATH: &str = "test-data/list-objects-v2.json";

#[tokio::test]
async fn sra_test() {
    tracing_subscriber::fmt::init();

    let conn = dvr::ReplayingConnection::from_file(LIST_BUCKETS_PATH).unwrap();

    let config = aws_sdk_s3::Config::builder()
        .credentials_provider(Credentials::for_tests())
        .region(Region::new("us-east-1"))
        .http_connector(DynConnector::new(conn.clone()))
        .build();
    let client = Client::from_conf(config);
    let fixup = FixupPlugin {
        timestamp: UNIX_EPOCH + Duration::from_secs(1624036048),
    };

    let resp = dbg!(
        client
            .list_objects_v2()
            .config_override(aws_sdk_s3::Config::builder().force_path_style(false))
            .bucket("test-bucket")
            .prefix("prefix~")
            .send_orchestrator_with_plugin(Some(fixup))
            .await
    );
    // To regenerate the test:
    // conn.dump_to_file("test-data/list-objects-v2.json").unwrap();
    let resp = resp.expect("valid e2e test");
    assert_eq!(resp.name(), Some("test-bucket"));
    conn.full_validate(MediaType::Xml).await.expect("failed")
}

struct FixupPlugin {
    timestamp: SystemTime,
}
impl RuntimePlugin for FixupPlugin {
    fn configure(
        &self,
        cfg: &mut ConfigBag,
        _interceptors: &mut Interceptors,
    ) -> Result<(), aws_smithy_runtime_api::client::runtime_plugin::BoxError> {
        cfg.set_request_time(RequestTime::new(self.timestamp.clone()));
        cfg.put(AwsUserAgent::for_tests());
        cfg.put(InvocationId::for_tests());
        Ok(())
    }
}
