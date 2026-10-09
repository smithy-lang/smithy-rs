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

//! End-to-end tests for the metrics pool's whole-child collision policy.
//!
//! Two independent middlewares each contribute their own metric to the request
//! pool. Because neither knows about the other, their fields can collide. The
//! pool compares children by their fully inflected field names: when two
//! children collide on any name, the later child wins and the earlier child is
//! discarded *in full* -- including fields the later child never emitted. This
//! prevents fields from separate contributors from being stitched into a record
//! that never existed. Giving each middleware a distinct prefix avoids the
//! collision. These tests drive both paths through [`MetricsLayer`] over a real
//! loopback connection.

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
use aws_smithy_http_server_metrics::MetricsPoolHandle;
use metrique::test_util::test_entry_sink;
use metrique::test_util::TestEntry;
use metrique::unit_of_work::metrics;
use tokio::sync::oneshot;
use tower::Layer;
use tower::Service;

type HttpRequest = http::Request<hyper::body::Incoming>;
type HttpResponse = http::Response<BoxBody>;

/// Metric contributed by `FirstMiddleware`. Emits two fields.
#[metrics]
#[derive(Default)]
struct FirstMetric {
    retry_count: u64,
    endpoint: &'static str,
}

/// Metric contributed by `SecondMiddleware`. Emits only the colliding field.
#[metrics]
#[derive(Default)]
struct SecondMetric {
    retry_count: u64,
}

/// Runs first and contributes [`FirstMetric`] to the request pool. An optional
/// prefix distinguishes its fields from other contributors.
#[derive(Clone)]
struct FirstMiddleware<S> {
    inner: S,
    prefix: Option<&'static str>,
}

impl<S> Service<HttpRequest> for FirstMiddleware<S>
where
    S: Service<HttpRequest, Response = HttpResponse, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
{
    type Response = HttpResponse;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: HttpRequest) -> Self::Future {
        let handle = pool_handle(&req);
        let metric = FirstMetric {
            retry_count: 1,
            endpoint: "first",
        };
        // A prefix namespaces this contributor's fields (`first_retry_count`),
        // so a producer controls its own emissions and cannot collide with
        // another contributor's identically named field.
        match self.prefix {
            Some(prefix) => handle.with_prefix([prefix]).append(metric),
            None => handle.append(metric),
        }
        let mut inner = self.inner.clone();
        Box::pin(async move { inner.call(req).await })
    }
}

/// Runs second and contributes [`SecondMetric`] to the request pool. An optional
/// prefix distinguishes its fields from other contributors.
#[derive(Clone)]
struct SecondMiddleware<S> {
    inner: S,
    prefix: Option<&'static str>,
}

impl<S> Service<HttpRequest> for SecondMiddleware<S>
where
    S: Service<HttpRequest, Response = HttpResponse, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
{
    type Response = HttpResponse;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: HttpRequest) -> Self::Future {
        let handle = pool_handle(&req);
        let metric = SecondMetric { retry_count: 2 };
        // A prefix namespaces this contributor's fields (`second_retry_count`),
        // so a producer controls its own emissions and cannot collide with
        // another contributor's identically named field.
        match self.prefix {
            Some(prefix) => handle.with_prefix([prefix]).append(metric),
            None => handle.append(metric),
        }
        let mut inner = self.inner.clone();
        Box::pin(async move { inner.call(req).await })
    }
}

/// Pull the pool handle from the request extensions.
fn pool_handle(req: &HttpRequest) -> MetricsPoolHandle {
    req.extensions()
        .get::<MetricsPoolHandle>()
        .expect("metrics pool handle is inserted into request extensions")
        .clone()
}

#[derive(Clone)]
struct Handler;

impl Service<HttpRequest> for Handler {
    type Response = HttpResponse;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: HttpRequest) -> Self::Future {
        Box::pin(async move {
            Ok(http::Response::builder()
                .status(200)
                .body(to_boxed("OK"))
                .unwrap())
        })
    }
}

/// Drive the two-middleware stack over a real loopback connection and return the
/// single per-request entry it emits to the test sink. When `prefixed` is set,
/// each middleware applies its own distinct prefix.
async fn drive(prefixed: bool) -> TestEntry {
    let (first_prefix, second_prefix) = if prefixed {
        (Some("first"), Some("second"))
    } else {
        (None, None)
    };

    let stack = FirstMiddleware {
        prefix: first_prefix,
        inner: SecondMiddleware {
            prefix: second_prefix,
            inner: Handler,
        },
    };

    let sink = test_entry_sink();
    let layer = MetricsLayer::new_with_sink(sink.sink.clone());
    let app = layer.layer(stack);

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

    let _ = shutdown_tx.send(());
    let _ = tokio::time::timeout(Duration::from_secs(2), server).await;

    let mut entries = sink.inspector.entries();
    assert_eq!(entries.len(), 1, "exactly one request entry is emitted");
    entries.remove(0)
}

#[tokio::test]
async fn colliding_middleware_discards_the_earlier_metric_in_full() {
    let entry = drive(false).await;

    // `SecondMiddleware` ran later, so its value wins the `retry_count` collision.
    assert_eq!(
        entry.metrics["retry_count"].as_u64(),
        2,
        "the later contributor's value wins the collision",
    );

    // `FirstMetric` is discarded in full, so its non-colliding `endpoint` field
    // never reaches the entry.
    assert!(
        !entry.values.contains_key("endpoint"),
        "the earlier contributor is dropped entirely, including its non-colliding fields",
    );
}

#[tokio::test]
async fn distinct_prefixes_keep_both_metrics() {
    let entry = drive(true).await;

    // Distinct prefixes mean no collision, so both contributors survive intact.
    assert_eq!(entry.metrics["first_retry_count"].as_u64(), 1);
    assert_eq!(entry.values["first_endpoint"], "first");
    assert_eq!(entry.metrics["second_retry_count"].as_u64(), 2);
}
