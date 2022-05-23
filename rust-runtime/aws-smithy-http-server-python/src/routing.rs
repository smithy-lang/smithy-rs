/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//!

use crate::error::BoxError;
use crate::state::PyHandlers;
use aws_smithy_http_server::body::{boxed, Body, BoxBody, HttpBody};
use aws_smithy_http_server::protocols::Protocol;
use aws_smithy_http_server::response::IntoResponse;
use aws_smithy_http_server::routing::{IntoMakeService, Route, Router, RouterFuture};
use aws_smithy_http_server::runtime_error::{RuntimeError, RuntimeErrorKind};
use delegate::delegate;
use http::{Request, Response, StatusCode};
use std::collections::HashMap;
use std::ops::Deref;
use std::{
    convert::Infallible,
    task::{Context, Poll},
};
use tower::layer::Layer;
use tower::util::ServiceExt;
use tower::{Service, ServiceBuilder};
use tower_http::map_response_body::MapResponseBodyLayer;

#[derive(Debug)]
pub struct PyRouter<B = Body> {
    router: Router<B>,
    py_handlers: PyHandlers,
}

impl<B> Deref for PyRouter<B> {
    type Target = Router<B>;

    fn deref(&self) -> &Self::Target {
        &self.router
    }
}

impl<B> PyRouter<B>
where
    B: Send + 'static,
{
    delegate! {
        to self.router {
        //     pub fn into_make_service(self) -> IntoMakeService<Router<B>>;
        //     pub fn layer<L, NewReqBody, NewResBody>(self, layer: L) -> Router<NewReqBody> where
        //         L: Layer<Route<B>>,
        //         L::Service: Service<Request<NewReqBody>, Response = Response<NewResBody>, Error = Infallible> + Clone + Send + 'static,
        //         <L::Service as Service<Request<NewReqBody>>>::Future: Send + 'static,
        //         NewResBody: HttpBody<Data = bytes::Bytes> + Send + 'static,
        //         NewResBody::Error: Into<BoxError>;
        }
    }
}

impl<B> Service<Request<B>> for PyRouter<B>
where
    B: Send + 'static,
{
    type Response = Response<BoxBody>;
    type Error = Infallible;
    type Future = RouterFuture<B>;

    #[inline]
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    #[inline]
    fn call(&mut self, req: Request<B>) -> Self::Future {
        self.router.call(req)
    }
}
