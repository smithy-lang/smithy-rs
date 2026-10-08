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

//! Regression test that a request cancelled mid-flight still emits its
//! per-request metrics entry through [`MetricsLayer`], with cancellation-time
//! child metrics flattened into it.
//!
//! The inner service's future never resolves. When the client drops the
//! connection, the server-side request future is cancelled (dropped) before it
//! completes. On drop it records a cancellation metric into the request's pool
//! via the [`MetricsPoolHandle`] it pulled from the request extensions. The
//! single per-request entry must still be emitted and must contain that metric.
//!
//! Coordination is deterministic: the inner future signals on a channel the
//! first time it is polled, so the test cancels the client only once the
//! server-side request future actually exists -- no timing guesses.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use aws_smithy_http_server::body::BoxBody;
use aws_smithy_http_server::routing::IntoMakeService;
use aws_smithy_http_server::serve::serve;
use aws_smithy_http_server_metrics::MetricsLayer;
use aws_smithy_http_server_metrics::MetricsPoolHandle;
use metrique::test_util::test_entry_sink;
use metrique::unit_of_work::metrics;
use tokio::sync::oneshot;
use tower::Layer;
use tower::Service;

type HttpRequest = http::Request<hyper::body::Incoming>;
type HttpResponse = http::Response<BoxBody>;

/// Child metric contributed only if the request is cancelled before completion.
#[metrics]
#[derive(Default)]
struct CancellationMetrics {
    cancelled: u64,
}

/// Inner service whose future never resolves. It pulls the pool handle from the
/// request extensions up front and, on drop (cancellation), appends a
/// cancellation metric to that handle.
#[derive(Clone)]
struct NeverCompletes {
    /// Fired the first time the inner future is polled, signalling the test that
    /// the server-side request future exists and may now be cancelled.
    polled_tx: std::sync::Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

/// Future that stays `Pending` and, when dropped before completing, records a
/// cancellation metric into the request's pool via the extension handle.
struct CancelRecordingFuture {
    handle: MetricsPoolHandle,
    polled_tx: std::sync::Arc<Mutex<Option<oneshot::Sender<()>>>>,
    completed: bool,
}

impl Future for CancelRecordingFuture {
    type Output = Result<HttpResponse, Infallible>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Signal (once) that the request future is in flight, then wait forever
        // so the only way out is cancellation.
        if let Some(tx) = self.polled_tx.lock().unwrap().take() {
            let _ = tx.send(());
        }
        Poll::Pending
    }
}

impl Drop for CancelRecordingFuture {
    fn drop(&mut self) {
        if !self.completed {
            self.handle
                .with_prefix(["cancel"])
                .append(CancellationMetrics { cancelled: 1 });
        }
    }
}

impl Service<HttpRequest> for NeverCompletes {
    type Response = HttpResponse;
    type Error = Infallible;
    type Future = CancelRecordingFuture;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: HttpRequest) -> Self::Future {
        let handle = req
            .extensions()
            .get::<MetricsPoolHandle>()
            .expect("metrics pool handle is inserted into request extensions")
            .clone();
        CancelRecordingFuture {
            handle,
            polled_tx: self.polled_tx.clone(),
            completed: false,
        }
    }
}

#[tokio::test]
async fn cancelled_request_still_emits_entry_with_cancellation_metric() {
    let sink = test_entry_sink();

    let (polled_tx, polled_rx) = oneshot::channel::<()>();
    let inner = NeverCompletes {
        polled_tx: std::sync::Arc::new(Mutex::new(Some(polled_tx))),
    };

    let layer = MetricsLayer::new_with_sink(sink.sink.clone());
    let app = layer.layer(inner);

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

    // Fire a request that the server will never answer.
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http::<http_body_util::Full<bytes::Bytes>>();
    let uri = format!("http://{addr}/test");
    let request = http::Request::builder()
        .uri(&uri)
        .body(http_body_util::Full::new(bytes::Bytes::from_static(
            b"body-bytes",
        )))
        .unwrap();
    let in_flight = tokio::spawn(async move { client.request(request).await });

    // Deterministically wait until the inner request future has actually been
    // polled, then cancel the client side.
    polled_rx
        .await
        .expect("inner future should be polled before the sender is dropped");
    in_flight.abort();

    // Shut the server down so the cancelled request future is dropped and the
    // per-request entry is emitted. The timeout is only a deadlock guard.
    let _ = shutdown_tx.send(());
    let _ = tokio::time::timeout(Duration::from_secs(2), server).await;

    let entries = sink.inspector.entries();
    assert_eq!(
        entries.len(),
        1,
        "a cancelled request must still emit exactly one entry",
    );

    let entry = &entries[0];
    assert!(
        entry.metrics.contains_key("cancel_cancelled"),
        "the cancellation metric contributed during drop must flatten into the entry",
    );
    assert_eq!(entry.metrics["cancel_cancelled"], 1);
}
