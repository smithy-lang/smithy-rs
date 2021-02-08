/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::SendOperationError;
use bytes::Bytes;
use smithy_http::middleware::load_response;
use smithy_http::operation;
use smithy_http::operation::Operation;
use smithy_http::response::ParseHttpResponse;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};
use tower::{BoxError, Layer, Service};

/// `ParseResponseService` dispatches [`Operation`](smithy_http::operation::Operation)s and parses them.
///
/// `ParseResponseService` is intended to wrap a `DispatchService` which will handle the interface between
/// services that operate on [`smithy_http::Request`](smithy_http::Request) and services that operate
/// on [`http::Request`](http::Request).
#[derive(Clone)]
pub struct ParseResponseService<S, O> {
    inner: S,
    _output_type: PhantomData<O>,
}

#[derive(Default)]
pub struct ParseResponseLayer<O> {
    _output_type: PhantomData<O>,
}

/// `ParseResponseLayer` dispatches [`Operation`](smithy_http::operation::Operation)s and parses them.
impl<O> ParseResponseLayer<O> {
    pub fn new() -> Self {
        ParseResponseLayer {
            _output_type: Default::default(),
        }
    }
}

impl<S, O> Layer<S> for ParseResponseLayer<O>
where
    S: Service<operation::Request>,
{
    type Service = ParseResponseService<S, O>;

    fn layer(&self, inner: S) -> Self::Service {
        ParseResponseService {
            inner,
            _output_type: Default::default(),
        }
    }
}

type BoxedResultFuture<T, E> = Pin<Box<dyn Future<Output = Result<T, E>>>>;

/// ParseResponseService
///
/// Generic Parameter Listing:
/// `S`: The inner service
/// `O`: The type of the response parser whose output type is `Result<T, E>`
/// `T`: The happy path return of the response parser
/// `E`: The error path return of the response parser
/// `B`: The HTTP Body type returned by the inner service
/// `R`: The type of the retry policy
impl<S, O, T, E, B, R> tower::Service<operation::Operation<O, R>> for ParseResponseService<S, O>
where
    S: Service<operation::Request, Response = http::Response<B>, Error = SendOperationError>,
    S::Future: 'static,
    B: http_body::Body + Unpin + From<Bytes> + 'static,
    B::Error: Into<BoxError>,
    O: ParseHttpResponse<B, Output = Result<T, E>> + 'static,
{
    type Response = smithy_http::result::SdkSuccess<T, B>;
    type Error = smithy_http::result::SdkError<E, B>;
    type Future = BoxedResultFuture<Self::Response, Self::Error>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(|err| err.into())
    }

    fn call(&mut self, req: Operation<O, R>) -> Self::Future {
        let (req, handler) = req.into_request_response();
        let resp = self.inner.call(req);
        let fut = async move {
            match resp.await {
                Err(e) => Err(e.into()),
                Ok(resp) => load_response(resp, &handler).await,
            }
        };
        Box::pin(fut)
    }
}
