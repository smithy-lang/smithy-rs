/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_config::retry::RetryConfig;
use aws_credential_types::Credentials;
use aws_sdk_s3::{Client, Region};
use aws_smithy_client::erase::DynConnector;
use aws_smithy_http::body::SdkBody;
use aws_smithy_types::timeout::TimeoutConfig;

fn get_default_config() -> impl std::future::Future<Output = aws_config::SdkConfig> {
    aws_config::from_env()
        .region(Region::from_static("us-west-2"))
        .credentials_provider(Credentials::from_keys(
            "access_key",
            "secret_key",
            Some("session_token".to_string()),
        ))
        .timeout_config(TimeoutConfig::disabled())
        .retry_config(RetryConfig::disabled())
        .http_connector(DynConnector::new(Adapter::default()))
        .load()
}

#[tokio::test]
async fn test_clients_from_service_config() {
    let shared_config = get_default_config().await;
    let client = Client::new(&shared_config);
    assert_eq!(client.conf().region().unwrap().to_string(), "us-west-2")
}

#[tokio::test]
async fn test_clients_list_buckets() {
    let shared_config = get_default_config().await;
    let client = Client::new(&shared_config);
    let result = client.list_buckets().send().await.unwrap();
    assert_eq!(result.buckets().unwrap().len(), 2)
}

#[derive(Default, Debug, Clone)]
struct Adapter {}

impl tower::Service<http::Request<SdkBody>> for Adapter {
    type Response = http::Response<SdkBody>;

    type Error = aws_smithy_http::result::ConnectorError;

    #[allow(clippy::type_complexity)]
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>,
    >;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: http::Request<SdkBody>) -> Self::Future {
        // Consumers here would pass the HTTP request to
        // the Wasm host in order to get the response back
        let body = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>
        <ListAllMyBucketsResult>
        <Buckets>
            <Bucket>
                <CreationDate>2023-01-23T11:59:03.575496Z</CreationDate>
                <Name>doc-example-bucket</Name>
            </Bucket>
            <Bucket>
                <CreationDate>2023-01-23T23:32:13.125238Z</CreationDate>
                <Name>doc-example-bucket2</Name>
            </Bucket>
        </Buckets>
        <Owner>
            <DisplayName>account-name</DisplayName>
            <ID>a3a42310-42d0-46d1-9745-0cee9f4fb851</ID>
        </Owner>
        </ListAllMyBucketsResult>";
        let res = http::Response::new(SdkBody::from(body));

        Box::pin(async move { Ok(res) })
    }
}
