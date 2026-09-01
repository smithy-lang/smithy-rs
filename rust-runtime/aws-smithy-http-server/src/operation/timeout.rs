/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::convert::Infallible;
use std::future::Future;
use std::task::{Context, Poll};
use std::time::Duration;

use http::{header, Request, Response, StatusCode, Version};
use tower::{Layer, Service};

/// Applies a timeout to an upgraded HTTP operation service.
///
/// When the timeout expires, the service returns `408 Request Timeout`.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct RequestTimeoutLayer {
    timeout: Duration,
    operation: &'static str,
}

impl RequestTimeoutLayer {
    /// Creates a new timeout layer.
    pub fn new(timeout: Duration, operation: &'static str) -> Self {
        Self { timeout, operation }
    }
}

impl<S> Layer<S> for RequestTimeoutLayer {
    type Service = RequestTimeout<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestTimeout {
            inner,
            timeout: self.timeout,
            operation: self.operation,
        }
    }
}

/// Service produced by [`RequestTimeoutLayer`].
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct RequestTimeout<S> {
    inner: S,
    timeout: Duration,
    operation: &'static str,
}

impl<B, S> Service<Request<B>> for RequestTimeout<S>
where
    S: Service<Request<B>, Response = Response<crate::body::BoxBody>, Error = Infallible>,
    S::Future: Send + 'static,
{
    type Response = Response<crate::body::BoxBody>;
    type Error = Infallible;
    type Future = RequestTimeoutFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let version = req.version();
        let request_id = request_id(&req);
        RequestTimeoutFuture {
            inner: self.inner.call(req),
            sleep: tokio::time::sleep(self.timeout),
            timeout: self.timeout,
            version,
            operation: self.operation,
            request_id,
        }
    }
}

#[cfg(feature = "request-id")]
fn request_id<B>(req: &Request<B>) -> Option<String> {
    req.extensions()
        .get::<crate::request::request_id::ServerRequestId>()
        .map(ToString::to_string)
}

#[cfg(not(feature = "request-id"))]
fn request_id<B>(_req: &Request<B>) -> Option<String> {
    None
}

pin_project_lite::pin_project! {
    /// Future produced by [`RequestTimeout`].
    #[doc(hidden)]
    pub struct RequestTimeoutFuture<F> {
        #[pin]
        inner: F,
        #[pin]
        sleep: tokio::time::Sleep,
        timeout: Duration,
        version: Version,
        operation: &'static str,
        request_id: Option<String>,
    }
}

impl<F> Future for RequestTimeoutFuture<F>
where
    F: Future<Output = Result<Response<crate::body::BoxBody>, Infallible>>,
{
    type Output = Result<Response<crate::body::BoxBody>, Infallible>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        if let Poll::Ready(result) = this.inner.poll(cx) {
            return Poll::Ready(result);
        }

        if this.sleep.poll(cx).is_ready() {
            #[cfg(feature = "request-id")]
            if let Some(request_id) = this.request_id.as_deref() {
                tracing::debug!(
                    operation = *this.operation,
                    request_id,
                    timeout_millis = this.timeout.as_millis(),
                    "request timed out"
                );
            } else {
                tracing::debug!(
                    operation = *this.operation,
                    timeout_millis = this.timeout.as_millis(),
                    "request timed out"
                );
            }
            #[cfg(not(feature = "request-id"))]
            tracing::debug!(
                operation = *this.operation,
                timeout_millis = this.timeout.as_millis(),
                "request timed out"
            );
            let mut response = Response::builder().status(StatusCode::REQUEST_TIMEOUT);
            if matches!(*this.version, Version::HTTP_10 | Version::HTTP_11) {
                response = response.header(header::CONNECTION, "close");
            }
            return Poll::Ready(Ok(response.body(crate::body::empty()).expect("valid response")));
        }

        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::service_fn;

    #[tokio::test]
    async fn returns_inner_response_before_timeout() {
        let mut service = RequestTimeoutLayer::new(Duration::from_secs(1), "test#Operation").layer(service_fn(
            |_req: Request<()>| async {
                Ok::<_, Infallible>(
                    Response::builder()
                        .status(StatusCode::OK)
                        .body(crate::body::empty())
                        .unwrap(),
                )
            },
        ));

        let response = service.call(Request::new(())).await.unwrap();
        assert_eq!(StatusCode::OK, response.status());
    }

    #[tokio::test]
    async fn returns_request_timeout_after_timeout() {
        let mut service = RequestTimeoutLayer::new(Duration::from_millis(1), "test#Operation").layer(service_fn(
            |_req: Request<()>| async {
                tokio::time::sleep(Duration::from_secs(1)).await;
                Ok::<_, Infallible>(
                    Response::builder()
                        .status(StatusCode::OK)
                        .body(crate::body::empty())
                        .unwrap(),
                )
            },
        ));

        let response = service.call(Request::new(())).await.unwrap();
        assert_eq!(StatusCode::REQUEST_TIMEOUT, response.status());
        assert_eq!("close", response.headers()[header::CONNECTION]);
    }

    #[tokio::test]
    async fn timeout_response_does_not_set_connection_close_for_http2() {
        let mut service = RequestTimeoutLayer::new(Duration::from_millis(1), "test#Operation").layer(service_fn(
            |_req: Request<()>| async {
                tokio::time::sleep(Duration::from_secs(1)).await;
                Ok::<_, Infallible>(
                    Response::builder()
                        .status(StatusCode::OK)
                        .body(crate::body::empty())
                        .unwrap(),
                )
            },
        ));
        let request = Request::builder().version(Version::HTTP_2).body(()).unwrap();

        let response = service.call(request).await.unwrap();
        assert_eq!(StatusCode::REQUEST_TIMEOUT, response.status());
        assert!(!response.headers().contains_key(header::CONNECTION));
    }
}
