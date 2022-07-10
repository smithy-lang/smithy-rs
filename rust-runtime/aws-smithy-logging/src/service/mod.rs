/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
mod sensitivity;

use std::{
    convert::Infallible,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::ready;
use http::{header::HeaderName, Request, Response};
use tower_service::Service;
use tracing::{debug, debug_span, instrument::Instrumented, Instrument};

use crate::{
    headers::{HeaderMarker, SensitiveHeaders},
    uri::SensitiveUri,
    OrFmt, Sensitive,
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
    Fut: Future<Output = Result<Response<T>, Infallible>>,
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

/// A middleware [`Service`](tower_service::Service) responsible for:
///     - Opening a [`tracing::debug_span`] for the lifetime of the request, which includes the operation name, the
///     [`Uri`](http::Uri), and the request headers.
///     - A [`tracing::debug`] during response, which includes the response status code and headers.
///
/// Data is marked as sensitive using the [`Sensitivity`] API and then passed to this via the
/// [`sensitivity`](InstrumentOperation::sensitivity) method. Using the `debug-logging` feature flag will ignore these
/// markings.
///
/// Data marked as sensitive will be replaced with [{redacted}](crate::REDACTED).
///
/// # Example
///
/// ```
/// # use aws_smithy_logging::{Sensitivity, InstrumentOperation};
/// # let svc = ();
/// let sensitivity = Sensitivity::new().path(|index| index == 1).status_code();
/// let mut svc = InstrumentOperation::new(svc, "foo-operation").sensitivity(sensitivity);
/// ```
#[derive(Debug)]
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
    pub fn sensitivity<NewSensitivity>(
        self,
        sensitivity: NewSensitivity,
    ) -> InstrumentOperation<Svc, NewSensitivity> {
        InstrumentOperation {
            inner: self.inner,
            operation_name: self.operation_name,
            sensitivity,
        }
    }
}

impl<Svc, U, V, RequestHeader, Path, QueryKey, ResponseHeader> Service<Request<U>>
    for InstrumentOperation<Svc, Sensitivity<RequestHeader, Path, QueryKey, ResponseHeader>>
where
    Svc: Service<Request<U>, Response = Response<V>, Error = Infallible>,
    RequestHeader: Fn(&HeaderName) -> HeaderMarker,
    Path: Fn(usize) -> bool,
    QueryKey: Fn(&str) -> bool,
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
            query_key,
            query,
            status_code,
            resp_header,
        } = &self.sensitivity;
        let headers = SensitiveHeaders::new(request.headers()).mark(&req_header);
        let uri = SensitiveUri::new(request.uri())
            .path(&path)
            .query_key(&query_key);
        let uri = if *query { uri.query() } else { uri };
        let span = debug_span!("request", operation = %self.operation_name, %uri, ?headers);

        LoggingFuture {
            inner: self.inner.call(request),

            status_code: *status_code,
            header: resp_header.clone(),
        }
        .instrument(span)
    }
}
