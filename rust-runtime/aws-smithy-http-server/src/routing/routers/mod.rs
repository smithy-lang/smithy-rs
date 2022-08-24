use std::{
    fmt::Debug,
    future::{ready, Future, Ready},
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::future::Either;
use http::Response;
use tower::{util::Oneshot, Service, ServiceExt};

pub mod aws_json;
// pub mod merged;
pub mod rest;

use crate::{
    body::{empty, BoxBody},
    response::IntoResponse,
};

const UNKNOWN_OPERATION_EXCEPTION: &str = "UnknownOperationException";

fn method_disallowed() -> http::Response<BoxBody> {
    http::Response::builder()
        .status(http::StatusCode::METHOD_NOT_ALLOWED)
        .body(empty())
        .expect("valid HTTP response")
}

pub trait Router<B> {
    type Service;
    type Error;

    fn match_route(&self, request: &http::Request<B>) -> Result<Self::Service, Self::Error>;
}

pub struct RoutingService<R, P> {
    router: R,
    _protocol: PhantomData<P>,
}

impl<R, P> Debug for RoutingService<R, P>
where
    R: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RoutingService")
            .field("router", &self.router)
            .field("_protocol", &self._protocol)
            .finish()
    }
}

impl<R, P> Clone for RoutingService<R, P>
where
    R: Clone,
{
    fn clone(&self) -> Self {
        Self {
            router: self.router.clone(),
            _protocol: PhantomData,
        }
    }
}

impl<R, P> RoutingService<R, P> {
    pub fn new(router: R) -> Self {
        Self {
            router,
            _protocol: PhantomData,
        }
    }

    pub fn map<RNew, F: FnOnce(R) -> RNew>(self, f: F) -> RoutingService<RNew, P> {
        RoutingService {
            router: f(self.router),
            _protocol: PhantomData,
        }
    }
}

type EitherOneshotReady<S, B> = Either<
    Oneshot<S, http::Request<B>>,
    Ready<Result<<S as Service<http::Request<B>>>::Response, <S as Service<http::Request<B>>>::Error>>,
>;

pin_project_lite::pin_project! {
    pub struct RoutingFuture<S, B> where S: Service<http::Request<B>> {
        #[pin]
        inner: EitherOneshotReady<S, B>
    }
}

impl<S, B> RoutingFuture<S, B>
where
    S: Service<http::Request<B>>,
{
    pub(super) fn from_oneshot(future: Oneshot<S, http::Request<B>>) -> Self {
        Self {
            inner: Either::Left(future),
        }
    }

    pub(super) fn from_response(response: S::Response) -> Self {
        Self {
            inner: Either::Right(ready(Ok(response))),
        }
    }
}

impl<S, B> Future for RoutingFuture<S, B>
where
    S: Service<http::Request<B>>,
{
    type Output = <S::Future as Future>::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().inner.poll(cx)
    }
}

impl<R, P, B> Service<http::Request<B>> for RoutingService<R, P>
where
    R: Router<B>,
    R::Service: Service<http::Request<B>, Response = http::Response<BoxBody>> + Clone,
    R::Error: IntoResponse<P>,
{
    type Response = Response<BoxBody>;
    type Error = <R::Service as Service<http::Request<B>>>::Error;
    type Future = RoutingFuture<R::Service, B>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<B>) -> Self::Future {
        let result = self.router.match_route(&req);
        match result {
            Ok(ok) => RoutingFuture::from_oneshot(ok.oneshot(req)),
            Err(err) => RoutingFuture::from_response(err.into_response()),
        }
    }
}
