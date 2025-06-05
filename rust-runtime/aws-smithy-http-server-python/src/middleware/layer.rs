/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Tower layer implementation of Python middleware handling.

use std::{
    convert::Infallible,
    marker::PhantomData,
    mem,
    task::{Context, Poll},
};

use aws_smithy_http_server::{
    body::{Body, BoxBody},
    response::IntoResponse,
};
use futures::{future::BoxFuture, TryFutureExt};
use http::{Request, Response};
use pyo3::Python;
use pyo3_async_runtimes::TaskLocals;
use tower::{util::BoxService, Layer, Service, ServiceExt};

use super::PyMiddlewareHandler;
use crate::{util::error::rich_py_err, PyMiddlewareException};

/// Tower [Layer] implementation of Python middleware handling.
///
/// Middleware stored in the `handler` attribute will be executed inside an async Tower middleware.
#[derive(Debug, Clone)]
pub struct PyMiddlewareLayer<P> {
    handler: PyMiddlewareHandler,
    locals: TaskLocals,
    _protocol: PhantomData<P>,
}

impl<P> PyMiddlewareLayer<P> {
    pub fn new(handler: PyMiddlewareHandler, locals: TaskLocals) -> Self {
        Self {
            handler,
            locals,
            _protocol: PhantomData,
        }
    }
}

impl<S, P> Layer<S> for PyMiddlewareLayer<P>
where
    PyMiddlewareException: IntoResponse<P>,
{
    type Service = PyMiddlewareService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        PyMiddlewareService::new(
            inner,
            self.handler.clone(),
            self.locals.clone(),
            PyMiddlewareException::into_response,
        )
    }
}

/// Tower [Service] wrapping the Python middleware [Layer].
#[derive(Clone, Debug)]
pub struct PyMiddlewareService<S> {
    inner: S,
    handler: PyMiddlewareHandler,
    locals: TaskLocals,
    into_response: fn(PyMiddlewareException) -> http::Response<BoxBody>,
}

impl<S> PyMiddlewareService<S> {
    pub fn new(
        inner: S,
        handler: PyMiddlewareHandler,
        locals: TaskLocals,
        into_response: fn(PyMiddlewareException) -> http::Response<BoxBody>,
    ) -> PyMiddlewareService<S> {
        Self {
            inner,
            handler,
            locals,
            into_response,
        }
    }
}

impl<S> Service<Request<Body>> for PyMiddlewareService<S>
where
    S: Service<Request<Body>, Response = Response<BoxBody>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send,
{
    type Response = S::Response;
    // We are making `Service` `Infallible` because we convert errors to responses via
    // `PyMiddlewareException::into_response` which has `IntoResponse<Protocol>` bound,
    // so we always return a protocol specific error response instead of erroring out.
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let inner = {
            // https://docs.rs/tower/latest/tower/trait.Service.html#be-careful-when-cloning-inner-services
            let clone = self.inner.clone();
            mem::replace(&mut self.inner, clone)
        };
        let handler = self.handler.clone();
        let handler_name = handler.name.clone();
        let next = BoxService::new(inner.map_err(|err| err.into()));
        let locals = self.locals.clone();
        let into_response = self.into_response;

        Box::pin(
            handler
                .call(req, next, locals)
                .or_else(move |err| async move {
                    tracing::error!(error = ?rich_py_err(Python::with_gil(|py| err.clone_ref(py))), handler_name, "middleware failed");
                    let response = (into_response)(err.into());
                    Ok(response)
                }),
        )
    }
}
