/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{
    future::Future,
    pin::Pin,
    task::{ready, Context, Poll},
};

use aws_smithy_async::time::SharedTimeSource;
use bytes::Bytes;
use tower::Service;

use crate::{
    body::{boxed, BoxBody},
    error::BoxError,
    service::ServiceShape,
};

use super::{
    body::MetricsFinalizingBody,
    state::{ConcurrentRequests, Finisher, MetricsState, ServiceIdentity},
    Outcome,
};

/// A [`Service`] that sets up per-request state for collecting standard server metrics, and
/// finishes the request once its response has been sent.
///
/// Generated services wrap their routing service in a `MetricsService`.
#[derive(Debug, Clone)]
pub struct MetricsService<S> {
    inner: S,
    enabled: bool,
    time_source: SharedTimeSource,
    service: ServiceIdentity,
    concurrent_requests: ConcurrentRequests,
}

impl<S> MetricsService<S> {
    /// Wraps `inner`, the routing service of the service `Ser`. Standard metrics are collected
    /// only if `enabled` is `true`.
    pub fn new<Ser: ServiceShape>(inner: S, enabled: bool) -> Self {
        Self {
            inner,
            enabled,
            time_source: SharedTimeSource::default(),
            service: ServiceIdentity::of::<Ser>(),
            concurrent_requests: ConcurrentRequests::default(),
        }
    }

    /// Maps the inner service using a closure.
    pub fn map<T>(self, f: impl FnOnce(S) -> T) -> MetricsService<T> {
        MetricsService {
            inner: f(self.inner),
            enabled: self.enabled,
            time_source: self.time_source,
            service: self.service,
            concurrent_requests: self.concurrent_requests,
        }
    }
}

impl<S, B, RespB> Service<http::Request<B>> for MetricsService<S>
where
    S: Service<http::Request<B>, Response = http::Response<RespB>>,
    RespB: http_body::Body<Data = Bytes> + Send + 'static,
    RespB::Error: Into<BoxError>,
{
    type Response = http::Response<BoxBody>;
    type Error = S::Error;
    type Future = MetricsServiceFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: http::Request<B>) -> Self::Future {
        let (concurrent_requests_count, concurrent_request) = if self.enabled {
            let (count, guard) = self.concurrent_requests.start();
            (count, Some(guard))
        } else {
            (0, None)
        };
        // Keeps any callbacks registered before the request reached the service.
        let state = MetricsState::take_over(
            &mut req,
            self.enabled,
            &self.time_source,
            &self.service,
            concurrent_requests_count,
        );
        super::record_request_id(&req);
        MetricsServiceFuture {
            inner: self.inner.call(req),
            finisher: Some(Finisher::new(state, concurrent_request)),
        }
    }
}

pin_project_lite::pin_project! {
    /// Response future for [`MetricsService`].
    pub struct MetricsServiceFuture<F> {
        #[pin]
        inner: F,
        // Moved into the response body. If there is no response, the request finishes when this is dropped.
        finisher: Option<Finisher>,
    }
}

impl<F, RespB, E> Future for MetricsServiceFuture<F>
where
    F: Future<Output = Result<http::Response<RespB>, E>>,
    RespB: http_body::Body<Data = Bytes> + Send + 'static,
    RespB::Error: Into<BoxError>,
{
    type Output = Result<http::Response<BoxBody>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let result = ready!(this.inner.poll(cx));
        let finisher = this.finisher.take().expect("polled after completion");
        Poll::Ready(result.map(|mut response| {
            let outcome = Outcome::take_from_response(&mut response);
            finisher.record_response(response.status(), outcome);
            response.map(|body| boxed(MetricsFinalizingBody::new(body, finisher)))
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        sync::{Arc, Mutex},
        time::{Duration, SystemTime},
    };

    use aws_smithy_async::time::StaticTimeSource;
    use bytes::Bytes;
    use http_body::Frame;
    use http_body_util::{BodyExt, Full};
    use tower::{service_fn, util::BoxCloneService, Service, ServiceExt};

    use crate::{
        metrics::{
            test_util::{TestOperation, TestService},
            Fault, ModeledOutcome, RequestExt, RequestMetrics, SuccessOutcome,
        },
        operation::OperationShape,
    };

    use super::*;

    /// Records the invocations of `on_finish` callbacks: callback index, and whether metrics were present.
    type Log = Arc<Mutex<Vec<(usize, bool)>>>;

    /// A service registering `callbacks` callbacks, each recording its invocation, then responding
    /// with `response`.
    fn service<RespB, E>(
        log: &Log,
        callbacks: usize,
        response: impl Fn() -> Result<http::Response<RespB>, E> + Clone,
    ) -> impl Service<http::Request<()>, Response = http::Response<RespB>, Error = E> {
        let log = log.clone();
        service_fn(move |mut req: http::Request<()>| {
            for index in 0..callbacks {
                let log = log.clone();
                req.on_finish(move |metrics| log.lock().unwrap().push((index, metrics.is_some())));
            }
            let response = response.clone();
            async move { response() }
        })
    }

    fn ok() -> Result<http::Response<Full<Bytes>>, Infallible> {
        Ok(http::Response::new(Full::new(Bytes::from_static(b"body"))))
    }

    fn logged(log: &Log) -> Vec<(usize, bool)> {
        log.lock().unwrap().clone()
    }

    #[tokio::test]
    async fn callbacks_run_in_order_once_body_is_fully_sent() {
        let log = Log::default();
        let svc = MetricsService::new::<TestService>(service(&log, 3, ok), true);
        let response = svc.oneshot(http::Request::new(())).await.unwrap();
        assert_eq!(logged(&log), [], "the body has not been sent yet");

        let mut body = response.into_body();
        let frame = body.frame().await.unwrap().unwrap();
        assert_eq!(frame.into_data().unwrap(), "body");
        assert_eq!(logged(&log), [], "the body has not been fully sent yet");

        assert!(body.frame().await.is_none());
        assert_eq!(logged(&log), [(0, true), (1, true), (2, true)]);

        drop(body);
        assert_eq!(logged(&log).len(), 3, "callbacks run only once");
    }

    #[tokio::test]
    async fn callbacks_receive_none_when_disabled() {
        let log = Log::default();
        let svc = MetricsService::new::<TestService>(service(&log, 2, ok), false);
        let response = svc.oneshot(http::Request::new(())).await.unwrap();
        response.into_body().collect().await.unwrap();
        assert_eq!(logged(&log), [(0, false), (1, false)]);
    }

    #[tokio::test]
    async fn callbacks_run_when_body_is_dropped_unsent() {
        let log = Log::default();
        let svc = MetricsService::new::<TestService>(service(&log, 1, ok), true);
        let response = svc.oneshot(http::Request::new(())).await.unwrap();
        drop(response);
        assert_eq!(logged(&log), [(0, true)]);
    }

    #[tokio::test]
    async fn callbacks_run_when_sending_body_fails() {
        struct FailingBody;
        impl http_body::Body for FailingBody {
            type Data = Bytes;
            type Error = std::io::Error;
            fn poll_frame(
                self: Pin<&mut Self>,
                _: &mut Context<'_>,
            ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
                Poll::Ready(Some(Err(std::io::Error::other("failed"))))
            }
        }

        let log = Log::default();
        let failing = || Ok::<_, Infallible>(http::Response::new(FailingBody));
        let svc = MetricsService::new::<TestService>(service(&log, 1, failing), true);
        let response = svc.oneshot(http::Request::new(())).await.unwrap();
        let mut body = response.into_body();
        assert!(body.frame().await.unwrap().is_err());
        assert_eq!(logged(&log), [(0, true)], "callbacks run before the body is dropped");
    }

    #[tokio::test]
    async fn callbacks_run_when_inner_service_fails() {
        let log = Log::default();
        let failing = || Err::<http::Response<Full<Bytes>>, _>("failed");
        let svc = MetricsService::new::<TestService>(service(&log, 1, failing), true);
        assert!(svc.oneshot(http::Request::new(())).await.is_err());
        assert_eq!(logged(&log), [(0, true)]);
    }

    #[tokio::test]
    async fn callbacks_run_when_response_future_is_dropped() {
        let log = Log::default();
        let registered = {
            let log = log.clone();
            service_fn(move |mut req: http::Request<()>| {
                let log = log.clone();
                req.on_finish(move |_| log.lock().unwrap().push((0, true)));
                std::future::pending::<Result<http::Response<Full<Bytes>>, Infallible>>()
            })
        };
        let mut svc = MetricsService::new::<TestService>(registered, true);
        let future = svc.ready().await.unwrap().call(http::Request::new(()));
        assert_eq!(logged(&log), []);
        drop(future);
        assert_eq!(logged(&log), [(0, true)]);
    }

    #[tokio::test]
    async fn panicking_callback_does_not_prevent_later_callbacks() {
        let log = Log::default();
        let inner = {
            let log = log.clone();
            service_fn(move |mut req: http::Request<()>| {
                req.on_finish(|_| panic!("callback panicked"));
                let log = log.clone();
                req.on_finish(move |_| log.lock().unwrap().push((1, true)));
                async { ok() }
            })
        };
        let svc = MetricsService::new::<TestService>(inner, true);
        let response = svc.oneshot(http::Request::new(())).await.unwrap();
        response.into_body().collect().await.unwrap();
        assert_eq!(logged(&log), [(1, true)]);
    }

    #[tokio::test(start_paused = true)]
    async fn metrics_span_handling_and_sending_the_response() {
        const HANDLING: Duration = Duration::from_secs(3);
        const SENDING: Duration = Duration::from_secs(5);
        const START_SYSTEM_TIME: SystemTime = SystemTime::UNIX_EPOCH;

        let seen = Arc::new(Mutex::new(None));
        let inner = {
            let seen = seen.clone();
            service_fn(move |mut req: http::Request<()>| {
                let seen = seen.clone();
                req.on_finish(move |metrics| *seen.lock().unwrap() = metrics);
                async {
                    tokio::time::advance(HANDLING).await;
                    ok()
                }
            })
        };
        let mut svc = MetricsService::new::<TestService>(inner, true);
        // The wall clock does not advance with tokio's paused clock, as it would not with a real clock either.
        svc.time_source = StaticTimeSource::new(START_SYSTEM_TIME).into();
        let response = svc.oneshot(http::Request::new(())).await.unwrap();
        let mut body = response.into_body();
        body.frame().await.unwrap().unwrap();
        tokio::time::advance(SENDING).await;
        assert!(body.frame().await.is_none());
        tokio::time::advance(Duration::from_secs(60)).await;
        drop(body);

        let metrics = seen.lock().unwrap().take().expect("callback received metrics");
        // Ends when the body is fully sent, not when it is dropped.
        let start = metrics.start_time();
        assert_eq!(metrics.end_time() - start.instant, HANDLING + SENDING);
        assert_eq!(start.system_time, START_SYSTEM_TIME);
    }

    #[tokio::test]
    async fn metrics_identify_the_service() {
        let seen = Arc::new(Mutex::new(None));
        let inner = {
            let seen = seen.clone();
            service_fn(move |mut req: http::Request<()>| {
                let seen = seen.clone();
                req.on_finish(move |metrics| *seen.lock().unwrap() = metrics);
                async { ok() }
            })
        };
        let response = MetricsService::new::<TestService>(inner, true)
            .oneshot(http::Request::new(()))
            .await
            .unwrap();
        drop(response);

        let metrics = seen.lock().unwrap().take().expect("callback received metrics");
        assert_eq!(metrics.service(), &TestService::ID);
        assert_eq!(metrics.service().name(), "TestService");
        assert_eq!(metrics.service_version(), Some("2026-11-12"));
    }

    /// Serves `response` through a `MetricsService` and returns the metrics received by an
    /// `on_finish` callback, after reading the body to its end (if `read_body`) or dropping it.
    async fn metrics_for<RespB>(response: http::Response<RespB>, read_body: bool) -> RequestMetrics
    where
        RespB: http_body::Body<Data = Bytes> + Send + Sync + 'static,
        RespB::Error: Into<BoxError>,
    {
        let seen = Arc::new(Mutex::new(None));
        let response = Arc::new(Mutex::new(Some(response)));
        let inner = {
            let seen = seen.clone();
            service_fn(move |mut req: http::Request<()>| {
                let seen = seen.clone();
                req.on_finish(move |metrics| *seen.lock().unwrap() = metrics);
                let response = response.lock().unwrap().take().unwrap();
                async { Ok::<_, Infallible>(response) }
            })
        };
        let response = MetricsService::new::<TestService>(inner, true)
            .oneshot(http::Request::new(()))
            .await
            .unwrap();
        assert!(
            response.extensions().get::<ModeledOutcome>().is_none(),
            "the marker is removed"
        );
        if read_body {
            let _ = response.into_body().collect().await;
        } else {
            drop(response);
        }
        let metrics = seen.lock().unwrap().take();
        metrics.expect("callback received metrics")
    }

    fn with_status(status: u16) -> http::Response<Full<Bytes>> {
        let mut response = http::Response::new(Full::new(Bytes::from_static(b"body")));
        *response.status_mut() = http::StatusCode::from_u16(status).unwrap();
        response
    }

    #[tokio::test]
    async fn unmarked_responses_are_classified_by_status() {
        for (status, result) in [(200, "success"), (302, "success"), (404, "client"), (503, "server")] {
            let metrics = metrics_for(with_status(status), true).await;
            let outcome = metrics.outcome().expect("a response was produced");
            assert_eq!(outcome.summary(), (result, "internal"), "{status}");
            assert_eq!(metrics.http().status_code().map(|code| code.as_u16()), Some(status));
        }
    }

    #[tokio::test]
    async fn modeled_outcome_takes_precedence_over_status() {
        let mut response = with_status(400);
        response
            .extensions_mut()
            .insert(ModeledOutcome::new(Err(Fault::Server)));
        let metrics = metrics_for(response, true).await;
        let outcome = metrics.outcome().expect("a response was produced");
        assert_eq!(outcome.summary(), ("server", "operation"));
    }

    #[tokio::test]
    async fn body_failure_is_a_server_error() {
        struct FailingBody;
        impl http_body::Body for FailingBody {
            type Data = Bytes;
            type Error = std::io::Error;
            fn poll_frame(
                self: Pin<&mut Self>,
                _: &mut Context<'_>,
            ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
                Poll::Ready(Some(Err(std::io::Error::other("failed"))))
            }
        }
        let mut response = http::Response::new(FailingBody);
        response
            .extensions_mut()
            .insert(ModeledOutcome::new(Ok(SuccessOutcome {})));
        let metrics = metrics_for(response, true).await;
        let outcome = metrics.outcome().expect("a response was produced");
        assert_eq!(outcome.summary(), ("server", "internal"));
        assert_eq!(
            metrics.http().status_code(),
            Some(http::StatusCode::OK),
            "the status sent before the body failed"
        );
    }

    #[tokio::test]
    async fn dropped_body_keeps_the_response_outcome() {
        let metrics = metrics_for(with_status(200), false).await;
        let outcome = metrics.outcome().expect("a response was produced");
        assert_eq!(outcome.summary(), ("success", "internal"));
    }

    #[tokio::test]
    async fn no_response_has_no_outcome() {
        let seen = Arc::new(Mutex::new(None));
        let inner = {
            let seen = seen.clone();
            service_fn(move |mut req: http::Request<()>| {
                let seen = seen.clone();
                req.on_finish(move |metrics| *seen.lock().unwrap() = metrics);
                async { Err::<http::Response<Full<Bytes>>, _>("failed") }
            })
        };
        let result = MetricsService::new::<TestService>(inner, true)
            .oneshot(http::Request::new(()))
            .await;
        assert!(result.is_err());
        let metrics = seen.lock().unwrap().take().expect("callback received metrics");
        assert!(metrics.outcome().is_none());
        assert!(metrics.http().status_code().is_none());
    }

    /// Serves a request for `uri` through a `MetricsService` around a REST `RoutingService` with
    /// one route, `GET /a`, for `TestOperation`. The route is wrapped in a layer that responds
    /// without calling it. Returns the metrics received by an `on_finish` callback registered
    /// before routing.
    async fn operation_metrics(uri: &str) -> RequestMetrics {
        use crate::protocol::{rest::router::RestRouter, rest_json_1::RestJson1};
        use crate::routing::{request_spec::*, RoutingService};

        let route = service_fn(|_: http::Request<()>| async { ok() });
        let router = RestRouter::from_routes([(
            RequestSpec::from_parts(
                http::Method::GET,
                vec![PathSegment::Literal(String::from("a"))],
                Vec::new(),
            ),
            <TestOperation as OperationShape>::ID,
            route,
        )])
        .layer(tower::layer::layer_fn(|_route| {
            service_fn(|_: http::Request<()>| async { ok() })
        }));
        let routing = RoutingService::<_, RestJson1>::new(router);

        let seen = Arc::new(Mutex::new(None));
        let inner = {
            let seen = seen.clone();
            service_fn(move |mut req: http::Request<()>| {
                let seen = seen.clone();
                req.on_finish(move |metrics| *seen.lock().unwrap() = metrics);
                routing.clone().oneshot(req)
            })
        };
        let response = MetricsService::new::<TestService>(inner, true)
            .oneshot(http::Request::get(uri).body(()).unwrap())
            .await
            .unwrap();
        drop(response);
        let metrics = seen.lock().unwrap().take();
        metrics.expect("callback received metrics")
    }

    #[tokio::test]
    async fn routed_requests_record_the_operation_even_if_a_layer_does_not_call_the_route() {
        let metrics = operation_metrics("/a").await;
        assert_eq!(metrics.operation(), Some(&<TestOperation as OperationShape>::ID));
    }

    #[tokio::test]
    async fn unrouted_requests_have_no_operation() {
        let metrics = operation_metrics("/unknown").await;
        assert_eq!(metrics.operation(), None);
    }

    /// A `MetricsService` whose requests record the concurrent requests count they observed, in
    /// the order they finish, under the request's `x-name` header.
    fn counting_service(
        enabled: bool,
        log: &Arc<Mutex<Vec<(String, usize)>>>,
    ) -> impl Service<http::Request<()>, Response = http::Response<BoxBody>, Error = Infallible> + Clone {
        let log = log.clone();
        let inner = service_fn(move |mut req: http::Request<()>| {
            let log = log.clone();
            let name = req.headers()["x-name"].to_str().unwrap().to_owned();
            req.on_finish(move |metrics| {
                let count = metrics.map_or(usize::MAX, |metrics| metrics.concurrent_requests_count());
                log.lock().unwrap().push((name, count));
            });
            async { ok() }
        });
        MetricsService::new::<TestService>(inner, enabled)
    }

    fn named(name: &str) -> http::Request<()> {
        http::Request::builder().header("x-name", name).body(()).unwrap()
    }

    #[tokio::test]
    async fn concurrent_requests_count_excludes_the_request_itself() {
        let log = Arc::default();
        let svc = counting_service(true, &log);
        // Responses whose bodies have not been sent keep their requests in progress.
        let a = svc.clone().oneshot(named("a")).await.unwrap();
        let b = svc.clone().oneshot(named("b")).await.unwrap();
        let c = svc.clone().oneshot(named("c")).await.unwrap();
        drop((a, b));
        drop(c);
        drop(svc.clone().oneshot(named("d")).await.unwrap());
        let logged = log.lock().unwrap().clone();
        assert_eq!(
            logged,
            [("a".into(), 0), ("b".into(), 1), ("c".into(), 2), ("d".into(), 0)]
        );
    }

    /// Serves `request` through a `MetricsService` around `wrap(route)`, and returns the request ID
    /// its metrics report. The route registers an `on_finish` callback.
    async fn reported_request_id<W, S>(request: http::Request<()>, wrap: W) -> Option<String>
    where
        W: FnOnce(BoxCloneService<http::Request<()>, http::Response<BoxBody>, Infallible>) -> S,
        S: Service<http::Request<()>, Response = http::Response<BoxBody>, Error = Infallible>,
    {
        let seen = Arc::new(Mutex::new(None));
        let route = {
            let seen = seen.clone();
            BoxCloneService::new(service_fn(move |mut req: http::Request<()>| {
                let seen = seen.clone();
                req.on_finish(move |metrics| *seen.lock().unwrap() = metrics);
                async { ok().map(|response| response.map(crate::body::boxed)) }
            }))
        };
        let response = MetricsService::new::<TestService>(wrap(route), true)
            .oneshot(request)
            .await
            .unwrap();
        drop(response);
        let metrics = seen.lock().unwrap().take();
        metrics.expect("callback received metrics").request_id()
    }

    #[tokio::test]
    async fn requests_without_an_id_report_none() {
        assert_eq!(reported_request_id(http::Request::new(()), |route| route).await, None);
    }

    #[cfg(feature = "request-id")]
    #[tokio::test]
    async fn an_id_assigned_around_the_service_is_reported() {
        use crate::request::request_id::ServerRequestIdProviderLayer;
        use tower::Layer;

        let seen = Arc::new(Mutex::new(None));
        let route = {
            let seen = seen.clone();
            service_fn(move |mut req: http::Request<()>| {
                let seen = seen.clone();
                req.on_finish(move |metrics| *seen.lock().unwrap() = metrics);
                async { ok().map(|response| response.map(crate::body::boxed)) }
            })
        };
        let service = ServerRequestIdProviderLayer::new().layer(MetricsService::new::<TestService>(route, true));
        drop(service.oneshot(http::Request::new(())).await.unwrap());
        let metrics = seen.lock().unwrap().take().expect("callback received metrics");
        assert!(metrics.request_id().is_some());
    }

    #[cfg(feature = "request-id")]
    #[tokio::test]
    async fn an_id_assigned_inside_the_service_is_reported() {
        use crate::request::request_id::ServerRequestIdProviderLayer;
        use tower::Layer;

        let reported = reported_request_id(http::Request::new(()), |route| {
            ServerRequestIdProviderLayer::new().layer(route)
        })
        .await;
        assert!(reported.is_some());
    }

    #[tokio::test]
    async fn callbacks_registered_before_the_service_are_adopted() {
        let log = Log::default();
        let metrics_service = MetricsService::new::<TestService>(service(&log, 1, ok), true);
        // A layer around the whole service, which registers a callback before the request reaches it.
        let outer = {
            let log = log.clone();
            let mut metrics_service = Some(metrics_service);
            tower::service_fn(move |mut req: http::Request<()>| {
                let log = log.clone();
                req.on_finish(move |metrics| log.lock().unwrap().push((99, metrics.is_some())));
                metrics_service.take().expect("called once").oneshot(req)
            })
        };
        let response = outer.oneshot(http::Request::new(())).await.unwrap();
        assert_eq!(logged(&log), [], "callbacks run once the request finishes");
        response.into_body().collect().await.unwrap();
        assert_eq!(
            logged(&log),
            [(99, true), (0, true)],
            "callbacks registered earlier run first, with the request's metrics"
        );
    }

    /// Serves `request` through a `MetricsService` whose route registers callback `index`, and
    /// fully sends the response.
    async fn serve_copy(log: &Log, index: usize, request: http::Request<()>) {
        let metrics_service = MetricsService::new::<TestService>(service(log, index + 1, ok), true);
        let response = metrics_service.oneshot(request).await.unwrap();
        response.into_body().collect().await.unwrap();
    }

    #[tokio::test]
    async fn the_first_copy_of_a_request_to_reach_a_service_gets_the_early_callbacks() {
        let log = Log::default();
        let mut first = http::Request::new(());
        first.on_finish({
            let log = log.clone();
            move |metrics| log.lock().unwrap().push((99, metrics.is_some()))
        });
        // A copy, as a retry or hedging layer around the service would make.
        let mut second = http::Request::new(());
        *second.extensions_mut() = first.extensions().clone();

        serve_copy(&log, 0, first).await;
        assert_eq!(
            logged(&log),
            [(99, true), (0, true)],
            "the early callback receives the first copy's metrics"
        );
        log.lock().unwrap().clear();
        serve_copy(&log, 1, second).await;
        assert_eq!(
            logged(&log),
            [(0, true), (1, true)],
            "the second copy gets a new state, without the early callback"
        );
    }

    #[test]
    fn map_preserves_enablement() {
        assert!(MetricsService::new::<TestService>((), true).map(|()| 0).enabled);
        assert!(!MetricsService::new::<TestService>((), false).map(|()| 0).enabled);
    }
}
