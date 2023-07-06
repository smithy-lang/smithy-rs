/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_config::SdkConfig;
use aws_credential_types::provider::SharedCredentialsProvider;
use aws_sdk_s3::config::{Credentials, Region};
use aws_sdk_s3::Client;
use aws_smithy_client::test_connection::capture_request;

use std::time::{Duration, UNIX_EPOCH};

#[tokio::test]
async fn test_s3_ops_are_customizable() {
    let (conn, rcvr) = capture_request(None);
    let sdk_config = SdkConfig::builder()
        .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
        .region(Region::new("us-east-1"))
        .http_connector(conn.clone())
        .build();

    let client = Client::new(&sdk_config);

    let op = client
        .list_buckets()
        .customize()
        .await
        .expect("list_buckets is customizable")
        .request_time_for_tests(UNIX_EPOCH + Duration::from_secs(1624036048))
        .user_agent_for_tests();

    // The response from the fake connection won't return the expected XML but we don't care about
    // that error in this test
    let _ = op
        .send()
        .await
        .expect_err("this will fail due to not receiving a proper XML response.");

    let expected_req = rcvr.expect_request();
    let auth_header = expected_req
        .headers()
        .get("Authorization")
        .unwrap()
        .to_owned();

    // This is a snapshot test taken from a known working test result
    let snapshot_signature =
        "Signature=c2028dc806248952fc533ab4b1d9f1bafcdc9b3380ed00482f9935541ae11671";
    assert!(
        auth_header
            .to_str()
            .unwrap()
            .contains(snapshot_signature),
        "authorization header signature did not match expected signature: got {}, expected it to contain {}",
        auth_header.to_str().unwrap(),
        snapshot_signature
    );
}

#[cfg(aws_sdk_orchestrator_mode)]
#[tokio::test]
async fn test_extract_metadata_via_customizable_operation() {
    // Interceptors aren’t supposed to store states, but it is done this way for a testing purpose.
    #[derive(Debug)]
    struct ExtractMetadataInterceptor(
        std::sync::Mutex<Option<std::sync::mpsc::Sender<(String, String)>>>,
    );

    impl aws_smithy_runtime_api::client::interceptors::Interceptor for ExtractMetadataInterceptor {
        fn modify_before_signing(
            &self,
            _context: &mut aws_smithy_runtime_api::client::interceptors::context::BeforeTransmitInterceptorContextMut<'_>,
            cfg: &mut aws_smithy_types::config_bag::ConfigBag,
        ) -> Result<(), aws_smithy_runtime_api::box_error::BoxError> {
            let metadata = cfg
                .load::<aws_smithy_http::operation::Metadata>()
                .expect("metadata should exist");
            let service_name = metadata.service().to_string();
            let operation_name = metadata.name().to_string();
            let tx = self.0.lock().unwrap().take().unwrap();
            tx.send((service_name.clone(), operation_name.clone()))
                .unwrap();
            Ok(())
        }
    }

    let (tx, rx) = std::sync::mpsc::channel();

    let (conn, _captured_request) = capture_request(None);
    let sdk_config = SdkConfig::builder()
        .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
        .region(Region::new("us-west-2"))
        .http_connector(conn)
        .build();
    let client = Client::new(&sdk_config);
    let _ = client
        .list_objects_v2()
        .bucket("test-bucket")
        .customize()
        .await
        .expect("operation should be customizable")
        .interceptor(ExtractMetadataInterceptor(std::sync::Mutex::new(Some(tx))))
        .send()
        .await;

    match rx.recv() {
        Ok((service_name, operation_name)) => {
            assert_eq!("s3", &service_name);
            assert_eq!("ListObjectsV2", &operation_name);
        }
        Err(_) => panic!(
            "failed to receive service name and operation name from `ExtractMetadataInterceptor`"
        ),
    }
}
