/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Body wrappers must pass through size_hint

use aws_config::SdkConfig;
use aws_sdk_s3::{
    config::{Credentials, HttpClient, Region, RuntimeComponents, SharedCredentialsProvider},
    primitives::{ByteStream, SdkBody},
    Client,
};
use aws_smithy_runtime_api::{
    client::{
        http::{HttpConnector, HttpConnectorFuture, HttpConnectorSettings, SharedHttpConnector},
        orchestrator::HttpRequest,
    },
    http::{Response, StatusCode},
};
use http_body::Body;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default)]
struct TestClient {
    response_body: Arc<Mutex<Option<SdkBody>>>,
    captured_body: Arc<Mutex<Option<SdkBody>>>,
}
impl HttpConnector for TestClient {
    fn call(&self, mut request: HttpRequest) -> HttpConnectorFuture {
        *self.captured_body.lock().unwrap() = Some(request.take_body());
        let body = self
            .response_body
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(SdkBody::empty);
        HttpConnectorFuture::ready(Ok(Response::new(StatusCode::try_from(200).unwrap(), body)))
    }
}
impl HttpClient for TestClient {
    fn http_connector(
        &self,
        _settings: &HttpConnectorSettings,
        _components: &RuntimeComponents,
    ) -> SharedHttpConnector {
        SharedHttpConnector::new(self.clone())
    }
}

#[tokio::test]
async fn download_body_size_hint_check() {
    let test_body_content = b"hello";
    let test_body = SdkBody::from(&test_body_content[..]);
    assert_eq!(
        Some(test_body_content.len() as u64),
        test_body.size_hint().exact(),
        "pre-condition check"
    );

    let http_client = TestClient {
        response_body: Arc::new(Mutex::new(Some(test_body))),
        ..Default::default()
    };
    let sdk_config = SdkConfig::builder()
        .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
        .region(Region::new("us-east-1"))
        .http_client(http_client)
        .build();
    let client = Client::new(&sdk_config);
    let response = client
        .get_object()
        .bucket("foo")
        .key("foo")
        .send()
        .await
        .unwrap();
    assert_eq!(
        (
            test_body_content.len() as u64,
            Some(test_body_content.len() as u64),
        ),
        response.body.size_hint(),
        "the size hint should be passed through all the default body wrappers"
    );
}

#[tokio::test]
async fn upload_body_size_hint_check() {
    let test_body_content = b"hello";

    let http_client = TestClient::default();
    let sdk_config = SdkConfig::builder()
        .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
        .region(Region::new("us-east-1"))
        .http_client(http_client.clone())
        .build();
    let client = Client::new(&sdk_config);
    let body = ByteStream::from_static(test_body_content);
    assert_eq!(
        (
            test_body_content.len() as u64,
            Some(test_body_content.len() as u64),
        ),
        body.size_hint(),
        "pre-condition check"
    );
    let _response = client
        .put_object()
        .bucket("foo")
        .key("foo")
        .body(body)
        .send()
        .await
        .unwrap();
    let captured_body = http_client.captured_body.lock().unwrap().take().unwrap();
    assert_eq!(
        Some(test_body_content.len() as u64),
        captured_body.size_hint().exact(),
        "the size hint should be passed through all the default body wrappers"
    );
}
