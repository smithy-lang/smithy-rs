/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// This test drives the modern (hyper 1.x) `aws-smithy-http-server` `serve` stack.
// The legacy server features alias `aws-smithy-http-server`, `http`, and `hyper`
// to their 0.x versions, which expose no `serve` entry point, so the imports
// below would not resolve under them (for example `--all-features` in CI).
#![cfg(not(any(
    feature = "aws-smithy-http-server-065",
    feature = "aws-smithy-legacy-http-server"
)))]

//! End-to-end test that a request driven through [`MetricsLayer`] over a real
//! (loopback) HTTP connection exposes a metrics pool both through the request
//! extensions and through the poll-scoped [`MetricsPool::current`], and that
//! child metrics contributed either way flatten into the single per-request
//! entry emitted to the sink.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use aws_smithy_http_server::body::{to_boxed, BoxBody};
use aws_smithy_http_server::routing::IntoMakeService;
use aws_smithy_http_server::serve::serve;
use aws_smithy_http_server_metrics::MetricsLayer;
use aws_smithy_http_server_metrics::MetricsPool;
use aws_smithy_http_server_metrics::MetricsPoolHandle;
use metrique::test_util::test_entry_sink;
use metrique::unit_of_work::metrics;
use tokio::sync::oneshot;
use tower::Layer;
use tower::Service;

type HttpRequest = http::Request<hyper::body::Incoming>;
type HttpResponse = http::Response<BoxBody>;

/// A heterogeneous child metric contributed by middleware/handlers.
#[metrics]
#[derive(Default)]
struct MiddlewareMetrics {
    suboperation: &'static str,
    retry_count: u64,
}

/// Fake inner service (stands in for the rest of the middleware/handler stack).
/// It contributes child metrics two ways:
/// 1. by pulling the [`MetricsPoolHandle`] out of the request extensions, and
/// 2. by discovering the current pool via [`MetricsPool::current`] during poll.
#[derive(Clone)]
struct Middleware;

impl Service<HttpRequest> for Middleware {
    type Response = HttpResponse;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: HttpRequest) -> Self::Future {
        // (1) Extension-delivered handle: middleware/handlers holding the request
        // can pull the producer handle and append explicitly.
        let ext_handle = req
            .extensions()
            .get::<MetricsPoolHandle>()
            .expect("metrics pool handle is inserted into request extensions")
            .clone();
        ext_handle.with_prefix(["ext"]).append(MiddlewareMetrics {
            suboperation: "FromExtension",
            retry_count: 1,
        });

        Box::pin(async move {
            // (2) Poll-scoped discovery: code running while the request future is
            // polled can find the pool without an explicit handle.
            MetricsPool::current()
                .expect("current pool is installed while the request future is polled")
                .with_prefix(["scoped"])
                .append(MiddlewareMetrics {
                    suboperation: "FromCurrent",
                    retry_count: 2,
                });

            Ok(http::Response::builder()
                .status(200)
                .body(to_boxed("OK"))
                .unwrap())
        })
    }
}

#[tokio::test]
async fn children_from_extension_and_current_flatten_into_the_request_entry() {
    let sink = test_entry_sink();

    // The layer builds a `SmithyMetrics` parent entry per request, appended to
    // the capturing test sink, and layers over the fake `Middleware`.
    let layer = MetricsLayer::new_with_sink(sink.sink.clone());
    let app = layer.layer(Middleware);

    // Drive the layered service over a real loopback connection so hyper produces
    // the `http::Request<Incoming>` the layer requires.
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
        .build_http();
    let uri = format!("http://{addr}/test");
    let request = http::Request::builder()
        .uri(&uri)
        .body(http_body_util::Full::<bytes::Bytes>::new(
            bytes::Bytes::from_static(b"body-bytes"),
        ))
        .unwrap();

    let response = client.request(request).await.expect("request failed");
    assert_eq!(response.status(), 200);

    // The per-request entry is emitted when the request future drops. Shut the
    // server down and wait for it so any lingering task state is released before
    // we inspect the sink.
    let _ = shutdown_tx.send(());
    let _ = tokio::time::timeout(Duration::from_secs(2), server).await;

    let entries = sink.inspector.entries();
    assert_eq!(entries.len(), 1, "exactly one request entry is emitted");
    let entry = &entries[0];

    // SmithyMetrics uses identity naming, which the pool children inherit.
    assert_eq!(entry.values["ext_suboperation"], "FromExtension");
    assert_eq!(entry.metrics["ext_retry_count"], 1);
    assert_eq!(entry.values["scoped_suboperation"], "FromCurrent");
    assert_eq!(entry.metrics["scoped_retry_count"], 2);
}
