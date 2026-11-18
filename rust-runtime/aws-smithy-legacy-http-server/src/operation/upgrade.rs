/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{
    convert::Infallible,
    future::{Future, Ready},
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures_util::ready;
use http::{header, StatusCode, Version};
use pin_project_lite::pin_project;
use tower::{util::Oneshot, Service, ServiceExt};
use tracing::error;

use crate::{
    body::BoxBody, plugin::Plugin, request::FromRequest, response::IntoResponse,
    runtime_error::InternalFailureException, service::ServiceShape, settings::RequestBodyReadTimeoutConfig,
};

use super::OperationShape;

/// A [`Plugin`] responsible for taking an operation [`Service`], accepting and returning Smithy
/// types and converting it into a [`Service`] taking and returning [`http`] types.
///
/// See [`Upgrade`].
#[derive(Debug, Clone)]
pub struct UpgradePlugin<Extractors> {
    _extractors: PhantomData<Extractors>,
    request_body_read_timeout: Option<RequestBodyReadTimeoutConfig>,
}

impl<Extractors> Default for UpgradePlugin<Extractors> {
    fn default() -> Self {
        Self {
            _extractors: PhantomData,
            request_body_read_timeout: None,
        }
    }
}

impl<Extractors> UpgradePlugin<Extractors> {
    /// Creates a new [`UpgradePlugin`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Selects the runtime body-read default appropriate to the modeled operation.
    #[doc(hidden)]
    pub fn with_default_request_body_read_timeout(mut self, payload: bool, operation: &'static str) -> Self {
        self.request_body_read_timeout = RequestBodyReadTimeoutConfig::default_for_operation(payload, operation);
        self
    }

    /// Applies a deadline while the request body is read and the operation input is constructed.
    ///
    /// The deadline is removed before the operation handler is invoked. Zero disables it.
    #[doc(hidden)]
    pub fn with_request_body_read_timeout(mut self, timeout: Duration, operation: &'static str) -> Self {
        self.request_body_read_timeout = RequestBodyReadTimeoutConfig::new(timeout, operation);
        self
    }
}

impl<Ser, Op, T, Extractors> Plugin<Ser, Op, T> for UpgradePlugin<Extractors>
where
    Ser: ServiceShape,
    Op: OperationShape,
{
    type Output = Upgrade<Ser::Protocol, (Op::Input, Extractors), T>;

    fn apply(&self, inner: T) -> Self::Output {
        Upgrade {
            _protocol: PhantomData,
            _input: PhantomData,
            inner,
            request_body_read_timeout: self.request_body_read_timeout,
        }
    }
}

/// A [`Service`] responsible for wrapping an operation [`Service`] accepting and returning Smithy
/// types, and converting it into a [`Service`] accepting and returning [`http`] types.
pub struct Upgrade<Protocol, Input, S> {
    _protocol: PhantomData<Protocol>,
    _input: PhantomData<Input>,
    inner: S,
    request_body_read_timeout: Option<RequestBodyReadTimeoutConfig>,
}

impl<P, Input, S> Clone for Upgrade<P, Input, S>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            _protocol: PhantomData,
            _input: PhantomData,
            inner: self.inner.clone(),
            request_body_read_timeout: self.request_body_read_timeout,
        }
    }
}

struct RequestBodyReadTimeoutState {
    // Created on the first pending poll rather than per request, so requests whose
    // input is ready immediately never need a timer. This keeps runtimes without a
    // tokio time driver working and avoids a per-request allocation.
    sleep: Option<Pin<Box<tokio::time::Sleep>>>,
    timeout: Duration,
    version: Version,
    operation: &'static str,
    #[cfg(feature = "request-id")]
    request_id: Option<String>,
}

impl RequestBodyReadTimeoutState {
    fn is_expired(&mut self, cx: &mut Context<'_>) -> bool {
        self.sleep.as_mut().is_some_and(|sleep| {
            // The timer driver may not have delivered expiry yet, so also check
            // the clock. Immediately ready inputs never allocate a timer.
            tokio::time::Instant::now() >= sleep.deadline() || sleep.as_mut().poll(cx).is_ready()
        })
    }

    fn response(&self) -> http::Response<crate::body::BoxBody> {
        #[cfg(feature = "request-id")]
        if let Some(request_id) = self.request_id.as_deref() {
            tracing::debug!(
                operation = self.operation,
                request_id,
                timeout_millis = self.timeout.as_millis(),
                "request body read timed out"
            );
        } else {
            tracing::debug!(
                operation = self.operation,
                timeout_millis = self.timeout.as_millis(),
                "request body read timed out"
            );
        }
        #[cfg(not(feature = "request-id"))]
        tracing::debug!(
            operation = self.operation,
            timeout_millis = self.timeout.as_millis(),
            "request body read timed out"
        );

        let mut response = http::Response::builder().status(StatusCode::REQUEST_TIMEOUT);
        if matches!(self.version, Version::HTTP_10 | Version::HTTP_11) {
            response = response.header(header::CONNECTION, "close");
        }
        response.body(crate::body::empty()).expect("valid response")
    }
}

#[cfg(feature = "request-id")]
fn request_id<B>(req: &http::Request<B>) -> Option<String> {
    req.extensions()
        .get::<crate::request::request_id::ServerRequestId>()
        .map(ToString::to_string)
}

pin_project! {
    #[project = InnerProj]
    #[project_replace = InnerProjReplace]
    enum Inner<FromFut, HandlerFut> {
        FromRequest {
            #[pin]
            inner: FromFut
        },
        Inner {
            #[pin]
            call: HandlerFut
        }
    }
}

type InnerAlias<Input, Protocol, B, S> = Inner<<Input as FromRequest<Protocol, B>>::Future, Oneshot<S, Input>>;

pin_project! {
    /// The [`Service::Future`] of [`Upgrade`].
    pub struct UpgradeFuture<Protocol, Input, B, S>
    where
        Input: FromRequest<Protocol, B>,
        S: Service<Input>,
    {
        service: Option<S>,
        #[pin]
        inner: InnerAlias<Input, Protocol, B, S>,
        request_body_read_timeout: Option<RequestBodyReadTimeoutState>,
    }
}

impl<P, Input, B, S> Future for UpgradeFuture<P, Input, B, S>
where
    Input: FromRequest<P, B>,
    <Input as FromRequest<P, B>>::Rejection: std::fmt::Display,
    S: Service<Input>,
    S::Response: IntoResponse<P>,
    S::Error: IntoResponse<P>,
{
    type Output = Result<http::Response<crate::body::BoxBody>, Infallible>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            let mut this = self.as_mut().project();
            let this2 = this.inner.as_mut().project();

            let call = match this2 {
                InnerProj::FromRequest { inner } => {
                    if let Some(timeout) = this.request_body_read_timeout {
                        if timeout.is_expired(cx) {
                            return Poll::Ready(Ok(timeout.response()));
                        }
                    }
                    let result = inner.poll(cx);
                    if let Some(timeout) = this.request_body_read_timeout {
                        if result.is_pending() && timeout.sleep.is_none() {
                            timeout.sleep = Some(Box::pin(tokio::time::sleep(timeout.timeout)));
                        }
                        // Input may become ready on the same poll that crosses an
                        // armed deadline. Expiry must win before calling the handler.
                        if timeout.is_expired(cx) {
                            return Poll::Ready(Ok(timeout.response()));
                        }
                    }
                    let result = match result {
                        Poll::Ready(result) => result,
                        Poll::Pending => return Poll::Pending,
                    };
                    *this.request_body_read_timeout = None;
                    match result {
                        Ok(ok) => this
                            .service
                            .take()
                            .expect("futures cannot be polled after completion")
                            .oneshot(ok),
                        Err(err) => {
                            // The error may arise either from a `FromRequest` failure for any user-defined
                            // handler's additional input parameters, or from a de-serialization failure
                            // of an input parameter specific to the operation.
                            tracing::trace!(error = %err, "parameter for the handler cannot be constructed");
                            return Poll::Ready(Ok(err.into_response()));
                        }
                    }
                }
                InnerProj::Inner { call } => {
                    let result = ready!(call.poll(cx));
                    let output = match result {
                        Ok(ok) => ok.into_response(),
                        Err(err) => err.into_response(),
                    };
                    return Poll::Ready(Ok(output));
                }
            };

            this.inner.as_mut().project_replace(Inner::Inner { call });
        }
    }
}

impl<P, Input, B, S> Service<http::Request<B>> for Upgrade<P, Input, S>
where
    Input: FromRequest<P, B>,
    <Input as FromRequest<P, B>>::Rejection: std::fmt::Display,
    S: Service<Input> + Clone,
    S::Response: IntoResponse<P>,
    S::Error: IntoResponse<P>,
{
    type Response = http::Response<crate::body::BoxBody>;
    type Error = Infallible;
    type Future = UpgradeFuture<P, Input, B, S>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // The check that the inner service is ready is done by `Oneshot` in `UpgradeFuture`'s
        // implementation.
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<B>) -> Self::Future {
        let clone = self.inner.clone();
        let service = std::mem::replace(&mut self.inner, clone);
        let request_body_read_timeout = self
            .request_body_read_timeout
            .map(|config| RequestBodyReadTimeoutState {
                sleep: None,
                timeout: config.timeout,
                version: req.version(),
                operation: config.operation,
                #[cfg(feature = "request-id")]
                request_id: request_id(&req),
            });
        UpgradeFuture {
            service: Some(service),
            inner: Inner::FromRequest {
                inner: <Input as FromRequest<P, B>>::from_request(req),
            },
            request_body_read_timeout,
        }
    }
}

/// A [`Service`] which always returns an internal failure message and logs an error.
#[derive(Copy)]
pub struct MissingFailure<P> {
    _protocol: PhantomData<fn(P)>,
}

impl<P> Default for MissingFailure<P> {
    fn default() -> Self {
        Self { _protocol: PhantomData }
    }
}

impl<P> Clone for MissingFailure<P> {
    fn clone(&self) -> Self {
        MissingFailure { _protocol: PhantomData }
    }
}

impl<R, P> Service<R> for MissingFailure<P>
where
    InternalFailureException: IntoResponse<P>,
{
    type Response = http::Response<BoxBody>;
    type Error = Infallible;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _request: R) -> Self::Future {
        error!("the operation has not been set");
        std::future::ready(Ok(InternalFailureException.into_response()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::task::Waker;
    use tokio::sync::oneshot;

    struct TestProtocol;
    struct TestInput;
    type InputResult = Result<TestInput, StatusCode>;
    type ResponseFuture = Pin<Box<dyn Future<Output = Result<http::Response<BoxBody>, Infallible>>>>;

    impl IntoResponse<TestProtocol> for StatusCode {
        fn into_response(self) -> http::Response<BoxBody> {
            http::Response::builder()
                .status(self)
                .body(crate::body::empty())
                .unwrap()
        }
    }

    impl FromRequest<TestProtocol, oneshot::Receiver<InputResult>> for TestInput {
        type Rejection = StatusCode;
        type Future = Pin<Box<dyn Future<Output = InputResult> + Send>>;

        fn from_request(request: http::Request<oneshot::Receiver<InputResult>>) -> Self::Future {
            Box::pin(async { request.into_body().await.expect("input sender must remain alive") })
        }
    }

    fn request(
        timeout: Option<Duration>,
        version: Version,
        handler_delay: Option<Duration>,
    ) -> (ResponseFuture, oneshot::Sender<InputResult>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let handler_calls = calls.clone();
        let mut upgrade = Upgrade::<TestProtocol, TestInput, _> {
            _protocol: PhantomData,
            _input: PhantomData,
            inner: tower::service_fn(move |_input| {
                handler_calls.fetch_add(1, Ordering::SeqCst);
                async move {
                    if let Some(delay) = handler_delay {
                        tokio::time::sleep(delay).await;
                    }
                    Ok::<_, Infallible>(StatusCode::OK)
                }
            }),
            request_body_read_timeout: timeout.map(|timeout| RequestBodyReadTimeoutConfig {
                timeout,
                operation: "test#Operation",
            }),
        };
        let (sender, receiver) = oneshot::channel();
        let request = http::Request::builder().version(version).body(receiver).unwrap();
        (Box::pin(upgrade.call(request)), sender, calls)
    }

    fn assert_pending(future: &mut ResponseFuture) {
        let mut cx = Context::from_waker(Waker::noop());
        assert!(future.as_mut().poll(&mut cx).is_pending());
    }

    #[tokio::test(start_paused = true)]
    async fn expired_deadline_rejects_ready_input() {
        for version in [Version::HTTP_11, Version::HTTP_2] {
            for succeeds in [true, false] {
                let (mut response, sender, calls) = request(Some(Duration::from_millis(10)), version, None);
                assert_pending(&mut response);
                tokio::time::advance(Duration::from_millis(10)).await;
                // The input is ready before the next poll, so expiry must win over
                // both successful deserialization and an input rejection.
                assert!(sender
                    .send(if succeeds {
                        Ok(TestInput)
                    } else {
                        Err(StatusCode::BAD_REQUEST)
                    })
                    .is_ok());
                let response = response.await.unwrap();
                assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
                assert_eq!(calls.load(Ordering::SeqCst), 0);
                if version == Version::HTTP_11 {
                    assert_eq!(response.headers()[header::CONNECTION], "close");
                } else {
                    assert!(!response.headers().contains_key(header::CONNECTION));
                }
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn input_ready_before_deadline_reaches_handler() {
        let (mut response, sender, calls) = request(Some(Duration::from_millis(10)), Version::HTTP_11, None);
        assert_pending(&mut response);
        tokio::time::advance(Duration::from_millis(9)).await;
        assert!(sender.send(Ok(TestInput)).is_ok());
        assert_eq!(response.await.unwrap().status(), StatusCode::OK);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn body_deadline_does_not_time_out_handler() {
        let (mut response, sender, calls) = request(
            Some(Duration::from_millis(10)),
            Version::HTTP_11,
            Some(Duration::from_millis(20)),
        );
        assert_pending(&mut response);
        tokio::time::advance(Duration::from_millis(9)).await;
        assert!(sender.send(Ok(TestInput)).is_ok());
        assert_pending(&mut response);
        tokio::time::advance(Duration::from_millis(100)).await;
        assert_eq!(response.await.unwrap().status(), StatusCode::OK);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn disabled_deadline_allows_late_input() {
        let (mut response, sender, calls) = request(None, Version::HTTP_11, None);
        assert_pending(&mut response);
        tokio::time::advance(Duration::from_secs(60)).await;
        assert!(sender.send(Ok(TestInput)).is_ok());
        assert_eq!(response.await.unwrap().status(), StatusCode::OK);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn immediately_ready_input_does_not_require_timer_driver() {
        let (mut response, sender, calls) = request(Some(Duration::from_millis(10)), Version::HTTP_11, None);
        assert!(sender.send(Ok(TestInput)).is_ok());
        let mut cx = Context::from_waker(Waker::noop());
        match response.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(response)) => assert_eq!(response.status(), StatusCode::OK),
            _ => panic!("immediately ready input should complete without a timer driver"),
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
