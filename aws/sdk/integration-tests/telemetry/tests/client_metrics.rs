/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_config::Region;
use aws_sdk_s3::config::{Credentials, SharedCredentialsProvider};
use aws_smithy_client_metrics::ClientMetrics;
use aws_smithy_runtime::client::http::test_util::{ReplayEvent, StaticReplayClient};
use aws_smithy_types::body::SdkBody;
use metrique::test_util::test_metric;
use metrique::unit_of_work::metrics;
use metrique_util::{with_metrics_pool, MetricsPool};

#[metrics(rename_all = "PascalCase")]
struct RequestMetrics {
    #[metrics(flatten)]
    sdk: MetricsPool,
}

#[tokio::test]
async fn client_metrics_installs_on_a_generated_sdk_client() {
    let http_client = StaticReplayClient::new(vec![ReplayEvent::new(
        http::Request::builder().body(SdkBody::empty()).unwrap(),
        http::Response::builder()
            .status(200)
            .body(SdkBody::empty())
            .unwrap(),
    )]);
    let sdk_config = aws_config::SdkConfig::builder()
        .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
        .region(Region::new("us-east-1"))
        .http_client(http_client)
        .build();
    let service_config = aws_sdk_s3::config::Builder::from(&sdk_config)
        .interceptor(ClientMetrics::new())
        .build();
    let client = aws_sdk_s3::Client::from_conf(service_config);
    let request = RequestMetrics {
        sdk: MetricsPool::new(),
    };

    with_metrics_pool(
        request.sdk.handle(),
        client
            .get_object()
            .bucket("test-bucket")
            .key("test-key")
            .send(),
    )
    .await
    .unwrap();

    let entry = test_metric(request);
    assert_eq!(entry.values["SdkS3GetObjectHttpStatusCode"], "200");
    assert_eq!(entry.metrics["SdkS3GetObjectSuccess"].as_u64(), 1);
}
