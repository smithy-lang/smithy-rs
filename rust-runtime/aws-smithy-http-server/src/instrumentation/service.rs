/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! A [`Service`] and it's associated [`Future`] providing sensitivity aware logging.

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{ready, TryFuture};
use http::{HeaderMap, Request, Response, StatusCode, Uri};
use tower::Service;
use tracing::{debug, debug_span, instrument::Instrumented, Instrument};

use crate::shape_id::ShapeId;

use super::{MakeDebug, MakeDisplay, MakeIdentity};

pin_project_lite::pin_project! {
    /// A [`Future`] responsible for logging the response status code and headers.
    struct InnerFuture<Fut, ResponseMakeFmt> {
        #[pin]
        inner: Fut,
        make: ResponseMakeFmt
    }
}

impl<Fut, ResponseMakeFmt, T> Future for InnerFuture<Fut, ResponseMakeFmt>
where
    Fut: TryFuture<Ok = Response<T>>,
    Fut: Future<Output = Result<Fut::Ok, Fut::Error>>,

    for<'a> ResponseMakeFmt: MakeDebug<&'a HeaderMap>,
    for<'a> ResponseMakeFmt: MakeDisplay<StatusCode>,
{
    type Output = Fut::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let response = ready!(this.inner.poll(cx))?;

        {
            let headers = this.make.make_debug(response.headers());
            let status_code = this.make.make_display(response.status());
            debug!(?headers, %status_code, "response");
        }

        Poll::Ready(Ok(response))
    }
}

// This is to provide type erasure.
pin_project_lite::pin_project! {
    /// An instrumented [`Future`] responsible for logging the response status code and headers.
    pub struct InstrumentedFuture<Fut, ResponseMakeFmt> {
        #[pin]
        inner: Instrumented<InnerFuture<Fut, ResponseMakeFmt>>
    }
}

impl<Fut, ResponseMakeFmt, T> Future for InstrumentedFuture<Fut, ResponseMakeFmt>
where
    Fut: TryFuture<Ok = Response<T>>,
    Fut: Future<Output = Result<Fut::Ok, Fut::Error>>,

    for<'a> ResponseMakeFmt: MakeDebug<&'a HeaderMap>,
    for<'a> ResponseMakeFmt: MakeDisplay<StatusCode>,
{
    type Output = Fut::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().inner.poll(cx)
    }
}

/// A middleware [`Service`] responsible for:
///   - Opening a [`tracing::debug_span`] for the lifetime of the request, which includes the operation name, the
///     [`Uri`], and the request headers.
///   - A [`tracing::debug`] during response, which includes the response status code and headers.
///
/// The [`Display`](std::fmt::Display) and [`Debug`] of the request and response components can be modified using
/// [`request_fmt`](InstrumentOperation::request_fmt) and [`response_fmt`](InstrumentOperation::response_fmt).
///
/// # Example
///
/// ```
/// # use aws_smithy_http_server::instrumentation::{sensitivity::{*, uri::*, headers::*}, *};
/// # use aws_smithy_http_server::shape_id::ShapeId;
/// # use tower::{Service, service_fn};
/// # use http::{Request, Response};
/// # async fn f(request: Request<()>) -> Result<Response<()>, ()> { Ok(Response::new(())) }
/// # let mut svc = service_fn(f);
/// # const ID: ShapeId = ShapeId::new("namespace#foo-operation", "namespace", "foo-operation");
/// let request_fmt = RequestFmt::new()
///     .label(|index| index == 1, None)
///     .query(|_| QueryMarker { key: false, value: true });
/// let response_fmt = ResponseFmt::new().status_code();
/// let mut svc = InstrumentOperation::new(svc, ID)
///     .request_fmt(request_fmt)
///     .response_fmt(response_fmt);
/// # svc.call(Request::new(()));
/// ```
#[derive(Debug, Clone)]
pub struct InstrumentOperation<S, RequestMakeFmt = MakeIdentity, ResponseMakeFmt = MakeIdentity> {
    inner: S,
    operation_id: ShapeId,
    make_request: RequestMakeFmt,
    make_response: ResponseMakeFmt,
}

impl<S> InstrumentOperation<S> {
    /// Constructs a new [`InstrumentOperation`] with no data redacted.
    pub fn new(inner: S, operation_id: ShapeId) -> Self {
        Self {
            inner,
            operation_id,
            make_request: MakeIdentity,
            make_response: MakeIdentity,
        }
    }
}

impl<S, RequestMakeFmt, ResponseMakeFmt> InstrumentOperation<S, RequestMakeFmt, ResponseMakeFmt> {
    /// Configures the request format.
    ///
    /// The argument is typically [`RequestFmt`](super::sensitivity::RequestFmt).
    pub fn request_fmt<R>(self, make_request: R) -> InstrumentOperation<S, R, ResponseMakeFmt> {
        InstrumentOperation {
            inner: self.inner,
            operation_id: self.operation_id,
            make_request,
            make_response: self.make_response,
        }
    }

    /// Configures the response format.
    ///
    /// The argument is typically [`ResponseFmt`](super::sensitivity::ResponseFmt).
    pub fn response_fmt<R>(self, make_response: R) -> InstrumentOperation<S, RequestMakeFmt, R> {
        InstrumentOperation {
            inner: self.inner,
            operation_id: self.operation_id,
            make_request: self.make_request,
            make_response,
        }
    }
}

impl<S, U, V, RequestMakeFmt, ResponseMakeFmt> Service<Request<U>>
    for InstrumentOperation<S, RequestMakeFmt, ResponseMakeFmt>
where
    S: Service<Request<U>, Response = Response<V>>,

    for<'a> RequestMakeFmt: MakeDebug<&'a HeaderMap>,
    for<'a> RequestMakeFmt: MakeDisplay<&'a Uri>,

    ResponseMakeFmt: Clone,
    for<'a> ResponseMakeFmt: MakeDebug<&'a HeaderMap>,
    for<'a> ResponseMakeFmt: MakeDisplay<StatusCode>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = InstrumentedFuture<S::Future, ResponseMakeFmt>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<U>) -> Self::Future {
        let span = {
            let headers = self.make_request.make_debug(request.headers());
            let uri = self.make_request.make_display(request.uri());
            debug_span!("request", operation = %self.operation_id.absolute(), method = %request.method(), %uri, ?headers)
        };

        InstrumentedFuture {
            inner: InnerFuture {
                inner: self.inner.call(request),
                make: self.make_response.clone(),
            }
            .instrument(span),
        }
    }
}
