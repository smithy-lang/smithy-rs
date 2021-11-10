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
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use crate::SdkError;
use aws_smithy_async::rt::sleep::AsyncSleep;
use aws_smithy_http::operation;
use tower::{Layer, Service};

/// Timeout Configuration
#[derive(Default, Debug, Clone)]
#[non_exhaustive]
pub struct Settings {
    connect_timeout: Option<Duration>,
    http_read_timeout: Option<Duration>,
    api_call_attempt_timeout: Option<Duration>,
    api_call_timeout: Option<Duration>,
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
        self.http_read_timeout
    }

    /// The configured TLS negotiation timeout
    pub fn tls_negotiation_timeout(&self) -> Option<Duration> {
        self.tls_negotiation_timeout
    }

    /// The configured HTTP request timeout per-attempt
    pub fn api_call_attempt_timeout(&self) -> Option<Duration> {
        self.api_call_attempt_timeout
    }

    /// The configured HTTP request timeout across all attempts
    pub fn api_call_timeout(&self) -> Option<Duration> {
        self.api_call_timeout
    }

    /// Sets the connect timeout
    pub fn with_connect_timeout(self, connect_timeout: Duration) -> Self {
        Self {
            connect_timeout: Some(connect_timeout),
            ..self
        }
    }

    /// Sets the read timeout
    pub fn with_read_timeout(self, http_read_timeout: Duration) -> Self {
        Self {
            http_read_timeout: Some(http_read_timeout),
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

    /// The configured HTTP request timeout per-attempt
    pub fn with_api_call_attempt_timeout(self, api_call_attempt_timeout: Duration) -> Self {
        Self {
            api_call_attempt_timeout: Some(api_call_attempt_timeout),
            ..self
        }
    }

    /// The configured HTTP request timeout across all attempts
    pub fn with_api_call_timeout(self, api_call_timeout: Duration) -> Self {
        Self {
            api_call_timeout: Some(api_call_timeout),
            ..self
        }
    }
}

/// A service that wraps another service, adding the ability to set a timeout for requests
/// handled by the inner service.
#[derive(Clone, Debug)]
pub struct TimeoutService<InnerService> {
    inner: InnerService,
    timeout: Option<(Arc<dyn AsyncSleep>, Duration)>,
}

impl<InnerService> TimeoutService<InnerService> {
    /// Given a function that will sleep a thread and timeout duration, create a new HttpRequestTimeout
    /// that will timeout if the inner service doesn't respond before the timeout elapses.
    pub fn new(inner: InnerService, sleep: Arc<dyn AsyncSleep>, timeout: Duration) -> Self {
        Self {
            inner,
            timeout: Some((sleep, timeout)),
        }
    }

    /// Create a new HttpRequestTimeout that will never timeout
    pub fn no_timeout(inner: InnerService) -> Self {
        Self {
            inner,
            timeout: None,
        }
    }

    // async fn response(
    //     inner_service_future: MaybeTimeoutFuture<InnerService::Future>,
    // ) -> Result<InnerService::Response, InnerService::Error> {
    //     inner_service_future
    //         .await
    //         .map_err(SdkError::ConstructionFailure)
    // }
}

/// A layer that wraps services in a timeout service
#[derive(Clone, Default, Debug)]
#[non_exhaustive]
pub struct TimeoutLayer {
    timeout: Option<Duration>,
    sleep_fn: Option<Arc<dyn AsyncSleep>>,
}

impl TimeoutLayer {
    /// Create a new HttpRequestTimeoutLayer
    pub fn new(sleep_fn: Option<Arc<dyn AsyncSleep>>, timeout: Option<Duration>) -> Self {
        TimeoutLayer {
            sleep_fn: sleep_fn,
            timeout,
        }
    }
}

impl<S> Layer<S> for TimeoutLayer
where
    S: Service<operation::Request, Response = operation::Response>,
{
    type Service = TimeoutService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        let timeout = if let (Some(sleep_fn), Some(timeout)) = (self.sleep_fn.clone(), self.timeout)
        {
            Some((sleep_fn, timeout))
        } else {
            None
        };

        TimeoutService { inner, timeout }
    }
}
/*
capture the error from the timeout future (will only come through if it's a timeout)
    will only come through if it times out
    can put it inside SdkError(ConstructionFailure)
need to copy the async block from ParseResponse because we need to map the error type which means
after unpacking the timeout future in the async block it will have to become a boxed future because
whatever the async block returns will be anonymous
 */

type BoxedResultFuture<T, E> = Pin<Box<dyn Future<Output = Result<T, E>> + Send>>;

impl<InnerService, Request, E> tower::Service<Request> for TimeoutService<InnerService>
where
    InnerService: Service<Request, Error = SdkError<E>>,
    InnerService::Future: Send + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    type Response = InnerService::Response;
    type Error = SdkError<E>;
    type Future = BoxedResultFuture<Self::Response, Self::Error>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, _req: Request) -> Self::Future {
        todo!()
        // use crate::hyper_impls::timeout_middleware::MaybeTimeoutFuture;
        // match &self.timeout {
        //     Some((sleep, duration)) => {
        //         let sleep = sleep.sleep(*duration);
        //         let timeout =
        //             aws_smithy_async::future::timeout::Timeout::new(self.inner.call(req), sleep);
        //
        //         MaybeTimeoutFuture::Timeout {
        //             timeout,
        //             error_type: "HTTP request timeout (multiple attempts)",
        //             duration: *duration,
        //         }
        //     }
        //     None => MaybeTimeoutFuture::NoTimeout {
        //         future: self.inner.call(req),
        //     },
        // }
    }
}
