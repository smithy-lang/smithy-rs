/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Timeout Configuration

use crate::SdkError;
use aws_smithy_async::future::timeout::Timeout;
use aws_smithy_async::rt::sleep::AsyncSleep;
use aws_smithy_types::timeout::OperationTimeoutConfig;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tower::Layer;

#[derive(Debug)]
struct RequestTimeoutError {
    kind: &'static str,
    duration: Duration,
}

impl RequestTimeoutError {
    pub fn new(kind: &'static str, duration: Duration) -> Self {
        Self { kind, duration }
    }
}

impl std::fmt::Display for RequestTimeoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} occurred after {:?}", self.kind, self.duration)
    }
}

impl std::error::Error for RequestTimeoutError {}

#[derive(Clone, Debug)]
/// A struct containing everything needed to create a new [`TimeoutService`]
pub struct TimeoutServiceParams {
    /// The duration of timeouts created from these params
    duration: Duration,
    /// The kind of timeouts created from these params
    kind: &'static str,
    /// The AsyncSleep impl that will be used to create time-limited futures
    async_sleep: Arc<dyn AsyncSleep>,
}

#[derive(Clone, Debug, Default)]
/// A struct of structs containing everything needed to create new [`TimeoutService`]s
pub(crate) struct ClientTimeoutParams {
    /// Params used to create a new API call [`TimeoutService`]
    pub(crate) operation_timeout: Option<TimeoutServiceParams>,
    /// Params used to create a new API call attempt [`TimeoutService`]
    pub(crate) operation_attempt_timeout: Option<TimeoutServiceParams>,
}

impl ClientTimeoutParams {
    pub fn new(
        timeout_config: &OperationTimeoutConfig,
        async_sleep: Option<Arc<dyn AsyncSleep>>,
    ) -> Self {
        if let Some(async_sleep) = async_sleep {
            Self {
                operation_timeout: timeout_config.operation_timeout().map(|duration| {
                    TimeoutServiceParams {
                        duration,
                        kind: "operation timeout (all attempts including retries)",
                        async_sleep: async_sleep.clone(),
                    }
                }),
                operation_attempt_timeout: timeout_config.operation_attempt_timeout().map(
                    |duration| TimeoutServiceParams {
                        duration,
                        kind: "operation attempt timeout (single attempt)",
                        async_sleep: async_sleep.clone(),
                    },
                ),
            }
        } else {
            Default::default()
        }
    }
}

/// A service that wraps another service, adding the ability to set a timeout for requests
/// handled by the inner service.
#[derive(Debug)]
pub struct TimeoutService<S> {
    inner: S,
    params: Option<TimeoutServiceParams>,
}

impl<S> Clone for TimeoutService<S>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            params: self.params.clone(),
        }
    }
}

impl<S> TimeoutService<S> {
    /// Create a new `TimeoutService` that will timeout after the duration specified in `params` elapses
    pub fn new(inner: S, params: Option<TimeoutServiceParams>) -> Self {
        Self { inner, params }
    }

    /// Create a new `TimeoutService` that will never timeout
    pub fn no_timeout(inner: S) -> Self {
        Self {
            inner,
            params: None,
        }
    }
}

/// A layer that wraps services in a timeout service
#[non_exhaustive]
#[derive(Debug)]
pub struct TimeoutLayer(Option<TimeoutServiceParams>);

impl TimeoutLayer {
    /// Create a new `TimeoutLayer`
    pub fn new(params: Option<TimeoutServiceParams>) -> Self {
        TimeoutLayer(params)
    }
}

impl<S> Layer<S> for TimeoutLayer {
    type Service = TimeoutService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TimeoutService {
            inner,
            params: self.0.clone(),
        }
    }
}

impl<Req, S, E> tower::Service<Req> for TimeoutService<S>
where
    S: tower::Service<Req, Error = SdkError<E>>,
    // 'static is required because we need to `Box::pin` this future
    S::Future: 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let future = self.inner.call(req);

        if let Some(params) = &self.params {
            let duration = params.duration;
            let async_sleep = params.async_sleep.clone();
            let kind = params.kind.clone();
            let timeout = Timeout::new(future, async_sleep.sleep(duration));

            Box::pin(async move {
                match timeout.await {
                    Ok(inner) => inner,
                    Err(_) => Err(SdkError::timeout_error(RequestTimeoutError::new(
                        kind, duration,
                    ))),
                }
            })
        } else {
            Box::pin(future)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::never::NeverService;
    use crate::{SdkError, TimeoutLayer};
    use aws_smithy_async::assert_elapsed;
    use aws_smithy_async::rt::sleep::{AsyncSleep, TokioSleep};
    use aws_smithy_http::body::SdkBody;
    use aws_smithy_http::operation::{Operation, Request};
    use aws_smithy_types::timeout::TimeoutConfig;
    use std::sync::Arc;
    use std::time::Duration;
    use tower::{Service, ServiceBuilder, ServiceExt};

    #[tokio::test]
    async fn test_timeout_service_ends_request_that_never_completes() {
        let req = Request::new(http::Request::new(SdkBody::empty()));
        let op = Operation::new(req, ());
        let never_service: NeverService<_, (), _> = NeverService::new();
        let timeout_config = OperationTimeoutConfig::from(
            TimeoutConfig::builder()
                .operation_timeout(Duration::from_secs_f32(0.25))
                .build(),
        );
        let sleep_impl: Arc<dyn AsyncSleep> = Arc::new(TokioSleep::new());
        let timeout_service_params = ClientTimeoutParams::new(&timeout_config, Some(sleep_impl));
        let mut svc = ServiceBuilder::new()
            .layer(TimeoutLayer::new(timeout_service_params.operation_timeout))
            .service(never_service);

        let now = tokio::time::Instant::now();
        tokio::time::pause();

        let err: SdkError<Box<dyn std::error::Error + 'static>> =
            svc.ready().await.unwrap().call(op).await.unwrap_err();

        assert_eq!(format!("{:?}", err), "TimeoutError(TimeoutError { source: RequestTimeoutError { kind: \"operation timeout (all attempts including retries)\", duration: 250ms } })");
        assert_elapsed!(now, Duration::from_secs_f32(0.25));
    }
}
