/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Tests that [`MetricsLayer`], wrapped around a service that collects standard server metrics,
//! publishes them as its default metrics.
//!
//! A real server is used because the layer serves hyper's request body type.

use std::convert::Infallible;
use std::time::Duration;

use aws_smithy_http_server::body::BoxBody;
use aws_smithy_http_server::metrics::MetricsService;
use aws_smithy_http_server::routing::IntoMakeService;
use aws_smithy_http_server::serve::serve;
use aws_smithy_http_server::service::ServiceShape;
use aws_smithy_http_server::shape_id::ShapeId;
use aws_smithy_http_server_metrics::MetricsLayer;
use metrique::test_util::{test_entry_sink, TestEntry};
use tokio::sync::oneshot;
use tower::Layer;

struct TestService;

impl ServiceShape for TestService {
    const ID: ShapeId = ShapeId::new("test#TestService", "test", "TestService");
    const VERSION: Option<&'static str> = Some("2026-11-19");
    type Protocol = ();
    type Operations = ();
}

const DEFAULT_METRIC_VALUES: [&str; 3] = ["service", "service_version", "http_status_code"];
const DEFAULT_METRIC_METRICS: [&str; 5] = [
    "outstanding_requests",
    "success",
    "client_error",
    "server_error",
    "operation_time",
];

/// The handler: responds with its status code.
#[derive(Clone)]
struct Respond(http::StatusCode);

impl tower::Service<http::Request<hyper::body::Incoming>> for Respond {
    type Response = http::Response<BoxBody>;
    type Error = Infallible;
    type Future = std::future::Ready<Result<Self::Response, Infallible>>;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Infallible>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: http::Request<hyper::body::Incoming>) -> Self::Future {
        let mut response = http::Response::new(aws_smithy_http_server::body::to_boxed("body"));
        *response.status_mut() = self.0;
        std::future::ready(Ok(response))
    }
}

/// Serves one request, which responds with `status`, through the layer built by `layer` wrapped
/// around a `MetricsService` collecting standard metrics if `collect`, and returns the emitted
/// entries.
async fn entries_for<L>(
    collect: bool,
    status: http::StatusCode,
    layer: impl FnOnce(metrique::writer::BoxEntrySink) -> L,
) -> Vec<TestEntry>
where
    L: Layer<MetricsService<Respond>>,
    L::Service: tower::Service<
            http::Request<hyper::body::Incoming>,
            Response = http::Response<BoxBody>,
            Error = Infallible,
        > + Clone
        + Send
        + 'static,
    <L::Service as tower::Service<http::Request<hyper::body::Incoming>>>::Future: Send,
{
    let sink = test_entry_sink();
    let service = MetricsService::new::<TestService>(Respond(status), collect);
    let app = layer(sink.sink.clone()).layer(service);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        serve(listener, IntoMakeService::new(app))
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
    });

    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http::<http_body_util::Empty<bytes::Bytes>>();
    let response = client
        .request(
            http::Request::builder()
                .uri(format!("http://{addr}/"))
                .body(http_body_util::Empty::new())
                .unwrap(),
        )
        .await
        .expect("request succeeds");
    assert_eq!(response.status(), status);
    http_body_util::BodyExt::collect(response.into_body())
        .await
        .expect("body is received");

    let _ = shutdown_tx.send(());
    let _ = tokio::time::timeout(Duration::from_secs(2), server).await;
    sink.inspector.entries()
}

fn only(entries: Vec<TestEntry>) -> TestEntry {
    assert_eq!(entries.len(), 1, "one entry per request: {entries:?}");
    entries.into_iter().next().unwrap()
}

#[tokio::test]
async fn publishes_standard_metrics() {
    let entry = only(entries_for(true, http::StatusCode::OK, MetricsLayer::new_with_sink).await);
    assert_eq!(entry.values["service"], "TestService");
    assert_eq!(entry.values["service_version"], "2026-11-19");
    assert_eq!(entry.values["http_status_code"], "200");
    assert_eq!(
        entry.metrics["outstanding_requests"], 1,
        "this request is counted"
    );
    assert_eq!(entry.metrics["success"], true);
    assert_eq!(entry.metrics["client_error"], false);
    assert_eq!(entry.metrics["server_error"], false);
    // The request was not routed to an operation, so no operation ran, and no request ID was
    // assigned.
    assert!(!entry.values.contains_key("operation"));
    assert!(!entry.metrics.contains_key("operation_time"));
    assert!(!entry.values.contains_key("request_id"));
}

#[tokio::test]
async fn classifies_failures() {
    let entry = only(
        entries_for(
            true,
            http::StatusCode::SERVICE_UNAVAILABLE,
            MetricsLayer::new_with_sink,
        )
        .await,
    );
    assert_eq!(entry.values["http_status_code"], "503");
    assert_eq!(entry.metrics["success"], false);
    assert_eq!(entry.metrics["client_error"], false);
    assert_eq!(entry.metrics["server_error"], true);
}

fn assert_no_default_metrics(entry: &TestEntry) {
    for name in DEFAULT_METRIC_VALUES {
        assert!(!entry.values.contains_key(name), "{name}");
    }
    for name in DEFAULT_METRIC_METRICS {
        assert!(!entry.metrics.contains_key(name), "{name}");
    }
}

#[tokio::test]
async fn publishes_no_default_metrics_unless_collected() {
    let entry = only(entries_for(false, http::StatusCode::OK, MetricsLayer::new_with_sink).await);
    assert_no_default_metrics(&entry);
}

#[tokio::test]
async fn publishes_no_default_metrics_when_disabled() {
    let entry = only(
        entries_for(true, http::StatusCode::OK, |sink| {
            use aws_smithy_http_server_metrics::layer::builder::DefaultMetricsBuildExt;
            MetricsLayer::builder()
                .init_metrics(move |_req| {
                    aws_smithy_http_server_metrics::default::SmithyMetrics::default()
                        .append_on_drop(sink.clone())
                })
                .disable_default_metrics()
                .build()
        })
        .await,
    );
    assert_no_default_metrics(&entry);
}

#[tokio::test]
async fn individual_default_metrics_can_be_disabled() {
    let entry = only(
        entries_for(true, http::StatusCode::OK, |sink| {
            use aws_smithy_http_server_metrics::layer::builder::DefaultMetricsBuildExt;
            MetricsLayer::builder()
                .init_metrics(move |_req| {
                    aws_smithy_http_server_metrics::default::SmithyMetrics::default()
                        .append_on_drop(sink.clone())
                })
                .disable_default_service_version_metric()
                .disable_default_operation_time_metric()
                .build()
        })
        .await,
    );
    assert!(!entry.values.contains_key("service_version"));
    assert!(!entry.metrics.contains_key("operation_time"));
    assert_eq!(
        entry.values["service"], "TestService",
        "other metrics are still published"
    );
}
