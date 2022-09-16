use std::{
    pin::Pin,
    task::{Context, Poll},
};

use aws_smithy_http_server::protocols::Protocol;
use futures::{ready, Future};
use http::{Request, Response};
use pin_project_lite::pin_project;
use pyo3_asyncio::TaskLocals;
use tower::{Layer, Service};

use super::PyMiddlewareTrait;

#[derive(Debug, Clone)]
pub struct PyMiddlewareLayer<T> {
    handler: T,
    protocol: Protocol,
    locals: TaskLocals,
}

impl<T> PyMiddlewareLayer<T> {
    pub fn new(handler: T, protocol: &str, locals: TaskLocals) -> PyMiddlewareLayer<T> {
        let protocol = match protocol {
            "aws.protocols#restJson1" => Protocol::RestJson1,
            "aws.protocols#restXml" => Protocol::RestXml,
            "aws.protocols#awsjson10" => Protocol::AwsJson10,
            "aws.protocols#awsjson11" => Protocol::AwsJson11,
            _ => panic!(),
        };
        Self {
            handler,
            protocol,
            locals,
        }
    }
}

impl<S, T> Layer<S> for PyMiddlewareLayer<T>
where
    T: Clone,
{
    type Service = PyMiddlewareService<S, T>;

    fn layer(&self, inner: S) -> Self::Service {
        PyMiddlewareService::new(inner, self.handler.clone(), self.protocol, self.locals.clone())
    }
}

#[derive(Clone, Debug)]
pub struct PyMiddlewareService<S, T> {
    inner: S,
    handler: T,
    protocol: Protocol,
    locals: TaskLocals
}

impl<S, T> PyMiddlewareService<S, T> {
    pub fn new(inner: S, handler: T, protocol: Protocol, locals: TaskLocals) -> PyMiddlewareService<S, T> {
        Self {
            inner,
            handler,
            protocol,
            locals
        }
    }

    pub fn layer(handler: T, protocol: &str, locals: TaskLocals) -> PyMiddlewareLayer<T> {
        PyMiddlewareLayer::new(handler, protocol, locals)
    }
}

impl<ReqBody, ResBody, S, M> Service<Request<ReqBody>> for PyMiddlewareService<S, M>
where
    M: PyMiddlewareTrait<ReqBody, ResponseBody = ResBody>,
    S: Service<Request<M::RequestBody>, Response = Response<ResBody>> + Clone,
{
    type Response = Response<ResBody>;
    type Error = S::Error;
    type Future = ResponseFuture<M, S, ReqBody>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let inner = self.inner.clone();
        let run = self.handler.run(req, self.protocol, self.locals.clone());

        ResponseFuture {
            middleware: State::Running { run },
            service: inner,
        }
    }
}

pin_project! {
    pub struct ResponseFuture<M, S, ReqBody>
    where
        M: PyMiddlewareTrait<ReqBody>,
        S: Service<Request<M::RequestBody>>,
    {
        #[pin]
        middleware: State<M::Future, S::Future>,
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

impl<M, S, ReqBody, B> Future for ResponseFuture<M, S, ReqBody>
where
    M: PyMiddlewareTrait<ReqBody, ResponseBody = B>,
    S: Service<Request<M::RequestBody>, Response = Response<B>>,
{
    type Output = Result<Response<B>, S::Error>;

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
