/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use aws_smithy_http_server::{
    body::{Body, BoxBody},
    protocols::Protocol,
};
use futures::{ready, Future};
use http::{Request, Response};
use pin_project_lite::pin_project;
use pyo3_asyncio::TaskLocals;
use tower::{Layer, Service};

use crate::{middleware::PyFuture, PyMiddlewares};

#[derive(Debug, Clone)]
pub struct PyMiddlewareLayer {
    handlers: PyMiddlewares,
    protocol: Protocol,
    locals: TaskLocals,
}

impl PyMiddlewareLayer {
    pub fn new(handlers: PyMiddlewares, protocol: &str, locals: TaskLocals) -> PyMiddlewareLayer {
        let protocol = match protocol {
            "aws.protocols#restJson1" => Protocol::RestJson1,
            "aws.protocols#restXml" => Protocol::RestXml,
            "aws.protocols#awsjson10" => Protocol::AwsJson10,
            "aws.protocols#awsjson11" => Protocol::AwsJson11,
            _ => panic!(),
        };
        Self {
            handlers,
            protocol,
            locals,
        }
    }
}

impl<S> Layer<S> for PyMiddlewareLayer {
    type Service = PyMiddlewareService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        PyMiddlewareService::new(
            inner,
            self.handlers.clone(),
            self.protocol,
            self.locals.clone(),
        )
    }
}

#[derive(Clone, Debug)]
pub struct PyMiddlewareService<S> {
    inner: S,
    handlers: PyMiddlewares,
    protocol: Protocol,
    locals: TaskLocals,
}

impl<S> PyMiddlewareService<S> {
    pub fn new(
        inner: S,
        handlers: PyMiddlewares,
        protocol: Protocol,
        locals: TaskLocals,
    ) -> PyMiddlewareService<S> {
        Self {
            inner,
            handlers,
            protocol,
            locals,
        }
    }

    pub fn layer(handlers: PyMiddlewares, protocol: &str, locals: TaskLocals) -> PyMiddlewareLayer {
        PyMiddlewareLayer::new(handlers, protocol, locals)
    }
}

impl<S> Service<Request<Body>> for PyMiddlewareService<S>
where
    S: Service<Request<Body>, Response = Response<BoxBody>> + Clone,
{
    type Response = Response<BoxBody>;
    type Error = S::Error;
    type Future = ResponseFuture<S>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let inner = self.inner.clone();
        let run = self.handlers.run(req, self.protocol, self.locals.clone());

        ResponseFuture {
            middleware: State::Running { run },
            service: inner,
        }
    }
}

pin_project! {
    pub struct ResponseFuture<S>
    where
        S: Service<Request<Body>>,
    {
        #[pin]
        middleware: State<PyFuture, S::Future>,
        service: S,
    }
}

pin_project! {
    #[project = StateProj]
    enum State<A, Fut> {
        Running {
            #[pin]
            run: A,
        },
        Done {
            #[pin]
            fut: Fut
        }
    }
}

impl<S> Future for ResponseFuture<S>
where
    S: Service<Request<Body>, Response = Response<BoxBody>>,
{
    type Output = Result<Response<BoxBody>, S::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        loop {
            match this.middleware.as_mut().project() {
                StateProj::Running { run } => {
                    let run = ready!(run.poll(cx));
                    match run {
                        Ok(req) => {
                            let fut = this.service.call(req);
                            this.middleware.set(State::Done { fut });
                        }
                        Err(res) => return Poll::Ready(Ok(res)),
                    }
                }
                StateProj::Done { fut } => return fut.poll(cx),
            }
        }
    }
}
