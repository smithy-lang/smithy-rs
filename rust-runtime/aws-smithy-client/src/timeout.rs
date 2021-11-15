/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Timeout Configuration
//!
//! While timeout configuration is unstable, this module is in aws-smithy-client.
//!
//! As timeout and HTTP configuration stabilizes, this will move to aws-types and become a part of
//! HttpSettings.
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use crate::SdkError;
use aws_smithy_async::future::timeout::Timeout;
use aws_smithy_async::rt::sleep::{default_async_sleep, AsyncSleep, Sleep, TokioSleep};
use aws_smithy_http::operation::Operation;
use pin_project_lite::pin_project;
use tower::Layer;

/// Timeout Configuration
#[derive(Default, Debug, Clone)]
#[non_exhaustive]
pub struct Settings {
    connect_timeout: Option<Duration>,
    read_timeout: Option<Duration>,
    tls_negotiation_timeout: Option<Duration>,
}

impl Settings {
    /// Create a new timeout configuration with no timeouts set
    pub fn new() -> Self {
        Default::default()
    }

    /// The configured TCP-connect timeout
    pub fn connect(&self) -> Option<Duration> {
        self.connect_timeout
    }

    /// The configured HTTP-read timeout
    pub fn read(&self) -> Option<Duration> {
        self.read_timeout
    }

    /// The configured TLS negotiation timeout
    pub fn tls_negotiation_timeout(&self) -> Option<Duration> {
        self.tls_negotiation_timeout
    }

    /// Sets the connect timeout
    pub fn with_connect_timeout(self, connect_timeout: Duration) -> Self {
        Self {
            connect_timeout: Some(connect_timeout),
            ..self
        }
    }

    /// Sets the read timeout
    pub fn with_read_timeout(self, read_timeout: Duration) -> Self {
        Self {
            read_timeout: Some(read_timeout),
            ..self
        }
    }

    /// The configured TLS negotiation timeout
    pub fn with_tls_negotiation_timeout(self, tls_negotiation_timeout: Duration) -> Self {
        Self {
            tls_negotiation_timeout: Some(tls_negotiation_timeout),
            ..self
        }
    }
}

/// A service that wraps another service, adding the ability to set a timeout for requests
/// handled by the inner service.
#[derive(Clone, Debug)]
pub struct TimeoutService<InnerService> {
    inner: InnerService,
    duration: Option<Duration>,
}

impl<InnerService> TimeoutService<InnerService> {
    /// Given a function that will sleep a thread and timeout duration, create a new HttpRequestTimeout
    /// that will timeout if the inner service doesn't respond before the timeout elapses.
    pub fn new(inner: InnerService, duration: Option<Duration>) -> Self {
        Self { inner, duration }
    }

    /// Create a new HttpRequestTimeout that will never timeout
    pub fn no_timeout(inner: InnerService) -> Self {
        Self {
            inner,
            duration: None,
        }
    }
}

/// A layer that wraps services in a timeout service
#[non_exhaustive]
#[derive(Debug)]
pub struct TimeoutLayer(Option<Duration>);

impl TimeoutLayer {
    /// Create a new HttpRequestTimeoutLayer
    pub fn new(duration: Option<Duration>) -> Self {
        TimeoutLayer(duration)
    }
}

impl<InnerService> Layer<InnerService> for TimeoutLayer {
    type Service = TimeoutService<InnerService>;

    fn layer(&self, inner: InnerService) -> Self::Service {
        TimeoutService {
            inner,
            duration: self.0,
        }
    }
}

pin_project! {
    #[non_exhaustive]
    #[must_use = "futures do nothing unless you `.await` or poll them"]
    /// A future representing a timeout timer. Wraps a [Timeout] with extra context for error reporting
    pub struct TimeoutLayerFuture<T> {
        #[pin]
        inner: Timeout<T, Sleep>
    }
}

impl<InnerFuture, T, E> Future for TimeoutLayerFuture<InnerFuture>
where
    InnerFuture: Future<Output = Result<T, SdkError<E>>>,
{
    type Output = Result<T, SdkError<E>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let me = self.project();
        match me.inner.poll(cx) {
            Poll::Ready(Ok(t)) => Poll::Ready(t),
            Poll::Ready(Err(timeout)) => {
                Poll::Ready(Err(SdkError::ConstructionFailure(timeout.into())))
            } // TODO: need to put this in the right place
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<H, R, InnerService, E> tower::Service<Operation<H, R>> for TimeoutService<InnerService>
where
    InnerService: tower::Service<Operation<H, R>, Error = SdkError<E>>,
{
    type Response = InnerService::Response;
    type Error = aws_smithy_http::result::SdkError<E>;
    type Future = TimeoutLayerFuture<InnerService::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Operation<H, R>) -> Self::Future {
        let base_future = self.inner.call(req);
        let timeout_future = self.duration.map_or_else(
            // If no timeout duration is provided, create a functionally infinite one
            || default_async_sleep().expect("No default sleep impl was found. This is unexpected. Please report this bug.").sleep(Duration::MAX),
            |duration| TokioSleep::new().sleep(duration),
        );
        let with_timeout = Timeout::new(base_future, timeout_future);
        TimeoutLayerFuture {
            inner: with_timeout,
        }
    }
}

#[cfg(test)]
mod test {
    use crate::never::NeverService;
    use crate::{SdkError, TimeoutLayer};
    use aws_smithy_http::body::SdkBody;
    use aws_smithy_http::operation::{Operation, Request};
    use std::time::Duration;
    use tower::{Service, ServiceBuilder, ServiceExt};

    // Copied from aws-smithy-client/src/hyper_impls.rs
    macro_rules! assert_elapsed {
        ($start:expr, $dur:expr) => {{
            let elapsed = $start.elapsed();
            // type ascription improves compiler error when wrong type is passed
            let lower: std::time::Duration = $dur;

            // Handles ms rounding
            assert!(
                elapsed >= lower && elapsed <= lower + std::time::Duration::from_millis(5),
                "actual = {:?}, expected = {:?}",
                elapsed,
                lower
            );
        }};
    }

    #[tokio::test]
    async fn test_timeout_service_ends_request_that_never_completes() {
        let req = Request::new(http::Request::new(SdkBody::empty()));
        let op = Operation::new(req, ());
        let never_service: NeverService<_, (), _> = NeverService::new();
        let mut svc = ServiceBuilder::new()
            .layer(TimeoutLayer::new(Some(Duration::from_secs_f32(0.25))))
            .service(never_service);

        let now = tokio::time::Instant::now();
        tokio::time::pause();

        let err: SdkError<Box<dyn std::error::Error + 'static>> =
            svc.ready().await.unwrap().call(op).await.unwrap_err();

        assert_eq!(format!("{:?}", err), "ConstructionFailure(TimedOutError)");
        assert_elapsed!(now, Duration::from_secs_f32(0.25));
    }
}
