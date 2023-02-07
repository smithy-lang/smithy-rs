/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::SendOperationError;
use aws_smithy_http::middleware::load_response;
use aws_smithy_http::operation;
use aws_smithy_http::operation::Operation;
use aws_smithy_http::response::ParseHttpResponse;
use aws_smithy_http::result::{SdkError, SdkSuccess};
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};
use tower::{Layer, Service};
use tracing::{debug_span, Instrument};

/// `ParseResponseService` dispatches [`Operation`](aws_smithy_http::operation::Operation)s and parses them.
///
/// `ParseResponseService` is intended to wrap a `DispatchService` which will handle the interface between
/// services that operate on [`operation::Request`](operation::Request) and services that operate
/// on [`http::Request`](http::Request).
pub struct ParseResponseService<S, H, R> {
    inner: S,
    _output_type: PhantomData<(H, R)>,
}

impl<S: Clone, H, R> Clone for ParseResponseService<S, H, R> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _output_type: PhantomData,
        }
    }
}

#[derive(Default)]
pub struct ParseResponseLayer<H, R> {
    _output_type: PhantomData<(H, R)>,
}

/// `ParseResponseLayer` dispatches [`Operation`](aws_smithy_http::operation::Operation)s and parses them.
impl<H, R> ParseResponseLayer<H, R> {
    pub fn new() -> Self {
        ParseResponseLayer {
            _output_type: Default::default(),
        }
    }
}

impl<S, H, R> Layer<S> for ParseResponseLayer<H, R> {
    type Service = ParseResponseService<S, H, R>;

    fn layer(&self, inner: S) -> Self::Service {
        ParseResponseService {
            inner,
            _output_type: Default::default(),
        }
    }
}

type BoxedResultFuture<T, E> = Pin<Box<dyn Future<Output = Result<T, E>> + Send>>;

/// ParseResponseService
///
/// Generic Parameter Listing:
/// `S`: The inner service
/// `H`: The type of the response parser whose output type is `Result<T, E>`
/// `T`: The happy path return of the response parser
/// `E`: The error path return of the response parser
/// `R`: The type of the retry policy
impl<S, H, T, E, R> Service<Operation<H, R>> for ParseResponseService<S, H, R>
where
    S: Service<operation::Request, Response = operation::Response, Error = SendOperationError>,
    S::Future: Send + 'static,
    H: ParseHttpResponse<Output = Result<T, E>> + Send + Sync + 'static,
    E: std::error::Error + 'static,
{
    type Response = SdkSuccess<T>;
    type Error = SdkError<E>;
    type Future = BoxedResultFuture<Self::Response, Self::Error>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(|err| err.into())
    }

    fn call(&mut self, req: Operation<H, R>) -> Self::Future {
        let (req, parts) = req.into_request_response();
        let handler = parts.response_handler;
        let resp = self.inner.call(req);
        Box::pin(async move {
            match resp.await {
                Err(e) => Err(e.into()),
                Ok(resp) => {
                    load_response(resp, &handler)
                        // load_response contains reading the body as far as is required & parsing the response
                        .instrument(debug_span!("load_response"))
                        .await
                }
            }
        })
    }
}
