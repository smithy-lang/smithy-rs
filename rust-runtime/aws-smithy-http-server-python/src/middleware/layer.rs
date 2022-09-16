use std::{task::{Context, Poll}, pin::Pin};

use futures::{Future, ready};
use http::{Request, Response};
use pin_project_lite::pin_project;
use tower::{Layer, Service};

#[derive(Debug, Clone)]
pub struct PyMiddlewareLayer<T> {
    handler: T,
}

impl<T> PyMiddlewareLayer<T> {
    pub fn new(handler: T) -> PyMiddlewareLayer<T> {
        Self { handler }
    }
}

impl<S, T> Layer<S> for PyMiddlewareLayer<T>
where
    T: Clone,
{
    type Service = PyMiddlewareService<S, T>;

    fn layer(&self, inner: S) -> Self::Service {
        PyMiddlewareService::new(inner, self.handler.clone())
    }
}

#[derive(Clone, Debug)]
pub struct PyMiddlewareService<S, T> {
    inner: S,
    handler: T,
}

impl<S, T> PyMiddlewareService<S, T> {
    pub fn new(inner: S, handler: T) -> PyMiddlewareService<S, T> {
        Self { inner, handler }
    }

    pub fn layer(handler: T) -> PyMiddlewareLayer<T> {
        PyMiddlewareLayer::new(handler)
    }
}

impl<ReqBody, ResBody, S, M> Service<Request<ReqBody>> for PyMiddlewareService<S, M>
where
    M: PyMiddleware<ReqBody, ResponseBody = ResBody>,
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
        let run = self.handler.run(req);

        ResponseFuture {
            middleware: State::Run { run },
            service: inner,
        }
    }
}

pin_project! {
    pub struct ResponseFuture<M, S, ReqBody>
    where
        M: PyMiddleware<ReqBody>,
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
        Run {
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
    M: PyMiddleware<ReqBody, ResponseBody = B>,
    S: Service<Request<M::RequestBody>, Response = Response<B>>,
{
    type Output = Result<Response<B>, S::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        loop {
            match this.middleware.as_mut().project() {
                StateProj::Run { run } => {
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

pub trait PyMiddleware<B> {
    type RequestBody;
    type ResponseBody;
    type Future: Future<Output = Result<Request<Self::RequestBody>, Response<Self::ResponseBody>>>;

    fn run(&mut self, request: Request<B>) -> Self::Future;
}

impl<B, F, Fut, ReqBody, ResBody> PyMiddleware<B> for F
where
    F: FnMut(Request<B>) -> Fut,
    Fut: Future<Output = Result<Request<ReqBody>, Response<ResBody>>>,
{
    type RequestBody = ReqBody;
    type ResponseBody = ResBody;
    type Future = Fut;

    fn run(&mut self, request: Request<B>) -> Self::Future {
        self(request)
    }
}
