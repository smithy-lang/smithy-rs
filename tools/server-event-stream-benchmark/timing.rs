// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Benchmark-only timing from HTTP service entry to serialized response frames.
//! Body completion means handed to Hyper, not flushed to a socket.
use hdrhistogram::Histogram;
use http::{Request, Response};
use http_body::{Body, Frame, SizeHint};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Instant;
use tower::Service;

#[derive(Default)]
pub struct Timings(Mutex<Option<Arc<Window>>>);

impl Timings {
    pub fn reset(&self) {
        // Requests retain their original window, so a late completion cannot
        // contaminate a later reset.
        *self.0.lock().unwrap() = Some(Arc::new(Window::default()));
    }

    pub fn snapshot(&self) -> Value {
        let window = self.0.lock().unwrap();
        window
            .as_ref()
            .map_or(Value::Null, |w| w.0.lock().unwrap().snapshot())
    }

    fn request(&self, path: &str) -> Option<RequestTiming> {
        let start = Instant::now();
        let window = self.0.lock().unwrap().clone()?;
        {
            let mut stats = window.0.lock().unwrap();
            stats.started += 1;
        }
        #[cfg(feature = "dial9")]
        let span = tracing::info_span!(
            target: "stream_benchmark", "server_request", dial9 = true,
            path, status = tracing::field::Empty,
            response_ready_ns = tracing::field::Empty,
            first_frame_ready_ns = tracing::field::Empty,
            body_complete_ns = tracing::field::Empty,
            outcome = tracing::field::Empty,
        );
        #[cfg(not(feature = "dial9"))]
        let _ = path;
        Some(RequestTiming {
            start,
            window,
            first: false,
            service_poll_ns: 0,
            body_poll_ns: 0,
            body_polls: 0,
            #[cfg(feature = "dial9")]
            span,
        })
    }
}

#[derive(Default)]
struct Window(Mutex<Stats>);

struct Stats {
    started: u64,
    ready: u64,
    completed: u64,
    errors: u64,
    aborted: u64,
    body_polls: u64,
    statuses: BTreeMap<u16, u64>,
    response_ready: Histogram<u64>,
    first_frame_ready: Histogram<u64>,
    body_complete: Histogram<u64>,
    service_poll_active: Histogram<u64>,
    body_poll_active: Histogram<u64>,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            started: 0,
            ready: 0,
            completed: 0,
            errors: 0,
            aborted: 0,
            body_polls: 0,
            statuses: BTreeMap::new(),
            response_ready: Histogram::new(3).unwrap(),
            first_frame_ready: Histogram::new(3).unwrap(),
            body_complete: Histogram::new(3).unwrap(),
            service_poll_active: Histogram::new(3).unwrap(),
            body_poll_active: Histogram::new(3).unwrap(),
        }
    }
}

fn distribution(h: &Histogram<u64>) -> Value {
    if h.is_empty() {
        return Value::Null;
    }
    json!({
        "samples": h.len(), "p50_ns": h.value_at_quantile(0.5),
        "p95_ns": h.value_at_quantile(0.95), "p99_ns": h.value_at_quantile(0.99),
        "max_ns": h.max(),
    })
}

impl Stats {
    fn snapshot(&self) -> Value {
        json!({
            "requests_started": self.started, "responses_ready": self.ready,
            "bodies_completed": self.completed, "errors": self.errors,
            "aborted": self.aborted,
            "active": self.started - self.completed - self.errors - self.aborted,
            "body_polls": self.body_polls, "statuses": self.statuses,
            "response_ready": distribution(&self.response_ready),
            "first_frame_ready": distribution(&self.first_frame_ready),
            "body_complete": distribution(&self.body_complete),
            "service_poll_active": distribution(&self.service_poll_active),
            "body_poll_active": distribution(&self.body_poll_active),
            "definition": "HTTP service entry to response ready, first serialized body frame, and body EOS handed to Hyper; excludes socket flush",
        })
    }
}

fn nanos(start: Instant) -> u64 {
    start.elapsed().as_nanos().min(u64::MAX as u128) as u64
}

#[derive(Clone, Copy)]
enum Outcome {
    Complete,
    Error,
    Aborted,
}

struct RequestTiming {
    start: Instant,
    window: Arc<Window>,
    first: bool,
    service_poll_ns: u64,
    body_poll_ns: u64,
    body_polls: u64,
    #[cfg(feature = "dial9")]
    span: tracing::Span,
}

impl RequestTiming {
    fn response_ready(&self, status: u16) {
        let elapsed = nanos(self.start);
        let mut stats = self.window.0.lock().unwrap();
        stats.ready += 1;
        *stats.statuses.entry(status).or_default() += 1;
        stats.response_ready.record(elapsed.max(1)).unwrap();
        #[cfg(feature = "dial9")]
        {
            self.span.record("status", status);
            self.span.record("response_ready_ns", elapsed);
        }
    }

    fn first_frame(&mut self) {
        if self.first {
            return;
        }
        self.first = true;
        let elapsed = nanos(self.start);
        self.window
            .0
            .lock()
            .unwrap()
            .first_frame_ready
            .record(elapsed.max(1))
            .unwrap();
        #[cfg(feature = "dial9")]
        self.span.record("first_frame_ready_ns", elapsed);
    }

    fn finish(self, outcome: Outcome) {
        let elapsed = nanos(self.start);
        let mut stats = self.window.0.lock().unwrap();
        stats.body_polls += self.body_polls;
        match outcome {
            Outcome::Complete => {
                stats.completed += 1;
                stats.body_complete.record(elapsed.max(1)).unwrap();
                stats
                    .service_poll_active
                    .record(self.service_poll_ns.max(1))
                    .unwrap();
                stats
                    .body_poll_active
                    .record(self.body_poll_ns.max(1))
                    .unwrap();
            }
            Outcome::Error => stats.errors += 1,
            Outcome::Aborted => stats.aborted += 1,
        }
        #[cfg(feature = "dial9")]
        {
            let _entered = self.span.enter();
            if matches!(outcome, Outcome::Complete) {
                self.span.record("body_complete_ns", elapsed);
            }
            self.span.record(
                "outcome",
                match outcome {
                    Outcome::Complete => "complete",
                    Outcome::Error => "error",
                    Outcome::Aborted => "aborted",
                },
            );
        }
    }
}

#[derive(Clone)]
pub struct TimingService<S> {
    inner: S,
    timings: Arc<Timings>,
}

impl<S> TimingService<S> {
    pub fn new(inner: S, timings: Arc<Timings>) -> Self {
        Self { inner, timings }
    }
}

impl<S, R, B> Service<Request<R>> for TimingService<S>
where
    S: Service<Request<R>, Response = Response<B>>,
    B: Body,
{
    type Response = Response<TimedBody<B>>;
    type Error = S::Error;
    type Future = TimingFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<R>) -> Self::Future {
        let timing = self.timings.request(request.uri().path());
        #[cfg(feature = "dial9")]
        let span = timing.as_ref().map(|t| t.span.clone());
        #[cfg(feature = "dial9")]
        let _entered = span.as_ref().map(tracing::Span::enter);
        TimingFuture {
            inner: self.inner.call(request),
            timing,
        }
    }
}

pin_project_lite::pin_project! {
    pub struct TimingFuture<F> {
        #[pin]
        inner: F,
        timing: Option<RequestTiming>,
    }
    impl<F> PinnedDrop for TimingFuture<F> {
        fn drop(this: Pin<&mut Self>) {
            if let Some(timing) = this.project().timing.take() {
                timing.finish(Outcome::Aborted);
            }
        }
    }
}

impl<F, B, E> Future for TimingFuture<F>
where
    F: Future<Output = Result<Response<B>, E>>,
    B: Body,
{
    type Output = Result<Response<TimedBody<B>>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let start = this.timing.as_ref().map(|_| Instant::now());
        #[cfg(feature = "dial9")]
        let span = this.timing.as_ref().map(|t| t.span.clone());
        #[cfg(feature = "dial9")]
        let _entered = span.as_ref().map(tracing::Span::enter);
        let result = this.inner.poll(cx);
        if let (Some(timing), Some(start)) = (this.timing.as_mut(), start) {
            timing.service_poll_ns += nanos(start);
        }
        match result {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(error)) => {
                if let Some(timing) = this.timing.take() {
                    timing.finish(Outcome::Error);
                }
                Poll::Ready(Err(error))
            }
            Poll::Ready(Ok(response)) => {
                let timing = this.timing.take();
                if let Some(timing) = &timing {
                    timing.response_ready(response.status().as_u16());
                }
                Poll::Ready(Ok(response.map(|inner| {
                    let mut body = TimedBody { inner, timing };
                    if body.inner.is_end_stream() {
                        if let Some(timing) = body.timing.take() {
                            timing.finish(Outcome::Complete);
                        }
                    }
                    body
                })))
            }
        }
    }
}

pin_project_lite::pin_project! {
    pub struct TimedBody<B> {
        #[pin]
        inner: B,
        timing: Option<RequestTiming>,
    }
    impl<B> PinnedDrop for TimedBody<B> {
        fn drop(this: Pin<&mut Self>) {
            if let Some(timing) = this.project().timing.take() {
                timing.finish(Outcome::Aborted);
            }
        }
    }
}

impl<B: Body> Body for TimedBody<B> {
    type Data = B::Data;
    type Error = B::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<B::Data>, B::Error>>> {
        let mut this = self.project();
        let start = this.timing.as_ref().map(|_| Instant::now());
        #[cfg(feature = "dial9")]
        let span = this.timing.as_ref().map(|t| t.span.clone());
        #[cfg(feature = "dial9")]
        let _entered = span.as_ref().map(tracing::Span::enter);
        let result = this.inner.as_mut().poll_frame(cx);
        if let (Some(timing), Some(start)) = (this.timing.as_mut(), start) {
            timing.body_poll_ns += nanos(start);
            timing.body_polls += 1;
        }
        if let Poll::Ready(Some(Ok(frame))) = &result {
            if frame.is_data() {
                if let Some(timing) = this.timing.as_mut() {
                    timing.first_frame();
                }
            }
        }
        let outcome = match &result {
            Poll::Ready(Some(Err(_))) => Some(Outcome::Error),
            Poll::Ready(None) => Some(Outcome::Complete),
            Poll::Ready(Some(Ok(_))) if this.inner.is_end_stream() => Some(Outcome::Complete),
            _ => None,
        };
        if let Some(outcome) = outcome {
            if let Some(timing) = this.timing.take() {
                timing.finish(outcome);
            }
        }
        result
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }
    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http_body_util::{BodyExt, Full};
    use std::convert::Infallible;
    use std::time::Duration;

    #[tokio::test]
    async fn response_future_does_not_end_stream_timing() {
        let timings = Arc::new(Timings::default());
        timings.reset();
        let body = http_body_util::StreamBody::new(async_stream::stream! {
            tokio::time::sleep(Duration::from_millis(20)).await;
            yield Ok::<_, Infallible>(Frame::data(Bytes::from_static(b"first")));
            tokio::time::sleep(Duration::from_millis(20)).await;
            yield Ok(Frame::data(Bytes::from_static(b"last")));
        });
        let response = TimingFuture {
            inner: std::future::ready(Ok::<_, Infallible>(Response::new(body))),
            timing: timings.request("/download"),
        }
        .await
        .unwrap();
        assert_eq!(timings.snapshot()["responses_ready"], 1);
        assert_eq!(timings.snapshot()["bodies_completed"], 0);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(bytes.as_ref(), b"firstlast");
        let result = timings.snapshot();
        assert_eq!(result["bodies_completed"], 1);
        assert_eq!(result["first_frame_ready"]["samples"], 1);
        assert!(result["body_complete"]["p50_ns"].as_u64().unwrap() >= 40_000_000);
        assert_eq!(result["body_poll_active"]["samples"], 1);
        assert!(result["body_polls"].as_u64().unwrap() >= 3);
    }

    #[tokio::test]
    async fn forwards_frames_status_and_headers_and_handles_final_frame() {
        let timings = Arc::new(Timings::default());
        timings.reset();
        let inner = tower::service_fn(|_: Request<()>| async {
            Ok::<_, Infallible>(
                Response::builder()
                    .status(201)
                    .header("x-test", "kept")
                    .body(Full::new(Bytes::from_static(b"unchanged")))
                    .unwrap(),
            )
        });
        let mut service = TimingService::new(inner, timings.clone());
        let response = service.call(Request::new(())).await.unwrap();
        assert_eq!(response.status(), 201);
        assert_eq!(response.headers()["x-test"], "kept");
        let mut body = Box::pin(response.into_body());
        assert_eq!(body.size_hint().exact(), Some(9));
        let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert_eq!(frame, Bytes::from_static(b"unchanged"));
        // Hyper may skip the extra EOS poll when is_end_stream is true.
        assert!(body.is_end_stream());
        assert_eq!(timings.snapshot()["bodies_completed"], 1);
        drop(body);
        assert_eq!(timings.snapshot()["aborted"], 0);
    }

    #[tokio::test]
    async fn reset_isolates_in_flight_requests_and_drops_are_aborted() {
        let timings = Arc::new(Timings::default());
        timings.reset();
        let response = TimingFuture {
            inner: std::future::ready(Ok::<_, Infallible>(Response::new(Full::new(
                Bytes::from_static(b"body"),
            )))),
            timing: timings.request("/old"),
        }
        .await
        .unwrap();
        timings.reset();
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            Bytes::from_static(b"body")
        );
        assert_eq!(timings.snapshot()["requests_started"], 0);
        let response = TimingFuture {
            inner: std::future::ready(Ok::<_, Infallible>(Response::new(Full::new(
                Bytes::from_static(b"body"),
            )))),
            timing: timings.request("/new"),
        }
        .await
        .unwrap();
        drop(response);
        assert_eq!(timings.snapshot()["aborted"], 1);
        assert_eq!(timings.snapshot()["body_complete"], Value::Null);
    }

    #[tokio::test]
    async fn empty_response_completes_without_polling() {
        let timings = Arc::new(Timings::default());
        timings.reset();
        let response = TimingFuture {
            inner: std::future::ready(Ok::<_, Infallible>(Response::new(http_body_util::Empty::<
                Bytes,
            >::new()))),
            timing: timings.request("/empty"),
        }
        .await
        .unwrap();
        assert!(response.body().is_end_stream());
        assert_eq!(timings.snapshot()["bodies_completed"], 1);
        assert_eq!(timings.snapshot()["first_frame_ready"], Value::Null);
        drop(response);
        assert_eq!(timings.snapshot()["aborted"], 0);
    }

    #[tokio::test]
    async fn trailers_and_body_errors_are_forwarded() {
        let timings = Arc::new(Timings::default());
        timings.reset();
        let mut trailers = http::HeaderMap::new();
        trailers.insert("x-trailer", "kept".parse().unwrap());
        let body = http_body_util::StreamBody::new(async_stream::stream! {
            yield Ok::<_, std::io::Error>(Frame::data(Bytes::from_static(b"data")));
            yield Ok(Frame::trailers(trailers));
            yield Err(std::io::Error::other("expected body failure"));
        });
        let response = TimingFuture {
            inner: std::future::ready(Ok::<_, Infallible>(Response::new(body))),
            timing: timings.request("/error"),
        }
        .await
        .unwrap();
        let mut body = Box::pin(response.into_body());
        assert_eq!(
            body.frame().await.unwrap().unwrap().into_data().unwrap(),
            Bytes::from_static(b"data")
        );
        assert_eq!(
            body.frame()
                .await
                .unwrap()
                .unwrap()
                .into_trailers()
                .unwrap()["x-trailer"],
            "kept"
        );
        assert_eq!(
            body.frame().await.unwrap().unwrap_err().to_string(),
            "expected body failure"
        );
        drop(body);
        let result = timings.snapshot();
        assert_eq!(result["errors"], 1);
        assert_eq!(result["bodies_completed"], 0);
        assert_eq!(result["aborted"], 0);
        assert_eq!(result["active"], 0);
    }
}
