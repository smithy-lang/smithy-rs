/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
mod sensitivity;

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{ready, TryFuture};
use http::{header::HeaderName, Request, Response};
use tower::Service;
use tracing::{debug, debug_span, instrument::Instrumented, Instrument};

use super::{
    headers::{HeaderMarker, SensitiveHeaders},
    uri::SensitiveUri,
    MakePath, OrFmt, QueryMarker, Sensitive,
};

pub use sensitivity::*;

pin_project_lite::pin_project! {
    /// An instrumented [`Future`] responsible for logging the response status code and headers.
    pub struct LoggingFuture<Fut, Header> {
        #[pin]
        inner: Fut,

        // Response sensitivity markers
        header: Header,
        status_code: bool
    }
}

impl<Fut, Header, T> Future for LoggingFuture<Fut, Header>
where
    Fut: TryFuture<Ok = Response<T>>,
    Fut: Future<Output = Result<Fut::Ok, Fut::Error>>,
    Header: Fn(&HeaderName) -> HeaderMarker + Clone,
{
    type Output = Fut::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let response = ready!(this.inner.poll(cx))?;

        let headers = SensitiveHeaders::new(response.headers()).mark(this.header.clone());
        let status_code = if *this.status_code {
            OrFmt::Left(Sensitive(response.status()))
        } else {
            OrFmt::Right(response.status())
        };

        debug!(?headers, %status_code, "response");
        Poll::Ready(Ok(response))
    }
}

/// A middleware [`Service`](tower::Service) responsible for:
///     - Opening a [`tracing::debug_span`] for the lifetime of the request, which includes the operation name, the
///     [`Uri`](http::Uri), and the request headers.
///     - A [`tracing::debug`] during response, which includes the response status code and headers.
///
/// Data is marked as sensitive using the [`Sensitivity`] API and then passed to this via the
/// [`sensitivity`](InstrumentOperation::sensitivity) method. Using the `unredacted-logging` feature flag will ignore these
/// markings.
///
/// Data marked as sensitive will be replaced with [{redacted}](crate::logging::REDACTED).
///
/// # Example
///
/// ```
/// # use aws_smithy_http_server::logging::{Sensitivity, InstrumentOperation};
/// # use tower::{Service, service_fn};
/// # use http::{Request, Response};
/// # async fn f(request: Request<()>) -> Result<Response<()>, ()> { Ok(Response::new(())) }
/// # let mut svc = service_fn(f);
/// let sensitivity = Sensitivity::new().path(|index| index == 1).greedy_path(3).status_code();
/// let mut svc = InstrumentOperation::new(svc, "foo-operation").sensitivity(sensitivity);
/// # svc.call(Request::new(()));
/// ```
#[derive(Debug, Clone)]
pub struct InstrumentOperation<Svc, Sensitivity = DefaultSensitivity> {
    inner: Svc,
    operation_name: &'static str,
    sensitivity: Sensitivity,
}

impl<Svc> InstrumentOperation<Svc> {
    /// Constructs a new [`InstrumentOperation`] with no data marked as sensitive.
    pub fn new(inner: Svc, operation_name: &'static str) -> Self {
        Self {
            inner,
            operation_name,
            sensitivity: Sensitivity::new(),
        }
    }
}

impl<Svc, Sensitivity> InstrumentOperation<Svc, Sensitivity> {
    /// Configures the data marked as sensitive.
    pub fn sensitivity<NewSensitivity>(self, sensitivity: NewSensitivity) -> InstrumentOperation<Svc, NewSensitivity> {
        InstrumentOperation {
            inner: self.inner,
            operation_name: self.operation_name,
            sensitivity,
        }
    }
}

impl<Svc, U, V, RequestHeader, Path, Query, ResponseHeader> Service<Request<U>>
    for InstrumentOperation<Svc, Sensitivity<RequestHeader, Path, Query, ResponseHeader>>
where
    Svc: Service<Request<U>, Response = Response<V>>,
    RequestHeader: Fn(&HeaderName) -> HeaderMarker,
    for<'a> Path: MakePath<'a>,
    Query: Fn(&str) -> QueryMarker,
    ResponseHeader: Fn(&HeaderName) -> HeaderMarker + Clone,
{
    type Response = Svc::Response;

    type Error = Svc::Error;

    type Future = Instrumented<LoggingFuture<Svc::Future, ResponseHeader>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<U>) -> Self::Future {
        let Sensitivity {
            req_header,
            path,
            query,
            status_code,
            resp_header,
        } = &self.sensitivity;
        let headers = SensitiveHeaders::new(request.headers()).mark(&req_header);
        let uri = SensitiveUri::new(request.uri()).make_path(&path).query(&query);
        let span = debug_span!("request", operation = %self.operation_name, method = %request.method(), %uri, ?headers);

        LoggingFuture {
            inner: self.inner.call(request),

            status_code: *status_code,
            header: resp_header.clone(),
        }
        .instrument(span)
    }
}
