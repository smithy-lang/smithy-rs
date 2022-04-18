use hyper::{Request, Response};
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};
use tower::Service;

#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct IntoMakeLambdaService<'a, F> {
    factory: F,
    _phantom: PhantomData<&'a ()>,
}

impl<'a, F> IntoMakeLambdaService<'a, F> {
    pub(super) fn new(factory: F) -> Self {
        Self {
            factory,
            _phantom: PhantomData,
        }
    }
}

// TODO Is the &'t lifetime useful given we don't make use of Target ?
impl<'a, 't, F, FFut, Svc, R, Target, MkErr> Service<&'t Target> for IntoMakeLambdaService<'a, F>
where
    F: Fn() -> FFut + Send + Clone, // TODO Send and Clone might not be useful
    FFut: Future<Output = Result<Svc, MkErr>> + Send,
    Svc: Service<lambda_http::Request, Response = R, Error = lambda_http::Error> + Send,
    Svc::Future: Send + 'a,
    R: lambda_http::IntoResponse,
    MkErr: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    type Error = MkErr;
    type Response = LambdaService<'a, Svc, R>;
    type Future = MkSvcFuture<'a, MkErr, FFut, R, Svc>;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _target: &'t Target) -> Self::Future {
        MkSvcFuture(Box::pin(self.factory.clone()()), PhantomData)
    }
}

pub struct MkSvcFuture<'a, E, F, R, S>(Pin<Box<F>>, PhantomData<&'a ()>)
where
    F: Future<Output = Result<S, E>> + Send,
    S: Service<lambda_http::Request, Response = R, Error = lambda_http::Error> + Send,
    S::Future: Send + 'a,
    E: Into<Box<dyn std::error::Error + Send + Sync>>,
    R: lambda_http::IntoResponse;

impl<'a, E, F, R, S> Future for MkSvcFuture<'a, E, F, R, S>
where
    F: Future<Output = Result<S, E>> + Send,
    S: Service<lambda_http::Request, Response = R, Error = lambda_http::Error> + Send,
    S::Future: Send + 'a,
    E: Into<Box<dyn std::error::Error + Send + Sync>>,
    R: lambda_http::IntoResponse,
{
    type Output = Result<LambdaService<'a, S, R>, E>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut TaskContext) -> Poll<Self::Output> {
        match self.0.as_mut().poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(service)) => Poll::Ready(Ok(LambdaService {
                service,
                _phantom_a: PhantomData,
            })),
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        }
    }
}

#[doc(hidden)]
pub struct LambdaService<'a, S, R>
where
    S: tower::Service<lambda_http::Request, Response = R, Error = lambda_http::Error> + Send,
    S::Future: Send + 'a,
    R: lambda_http::IntoResponse,
{
    service: S,
    _phantom_a: PhantomData<&'a ()>,
}

impl<'a, S, R> Service<Request<hyper::Body>> for LambdaService<'a, S, R>
where
    S: tower::Service<lambda_http::Request, Response = R, Error = lambda_http::Error> + Send,
    S::Future: Send + 'a,
    R: lambda_http::IntoResponse,
{
    type Response = Response<hyper::Body>;
    type Error = lambda_http::Error;

    type Future = TransformResponse<'a, R, lambda_http::Error>;

    fn poll_ready(&mut self, _cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<hyper::Body>) -> Self::Future {
        let req = hyper_to_lambda_request(req);
        let fut = Box::pin(self.service.call(req));

        TransformResponse { fut }
    }
}

/// Future that will convert a [`lambda_http::Response`] into a [`hyper::Response`]
pub struct TransformResponse<'a, R, E> {
    fut: Pin<Box<dyn Future<Output = Result<R, E>> + Send + 'a>>,
}

impl<'a, R, E> Future for TransformResponse<'a, R, E>
where
    R: lambda_http::IntoResponse,
    E: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    type Output = Result<hyper::Response<hyper::Body>, E>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut TaskContext) -> Poll<Self::Output> {
        match self.fut.as_mut().poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(result) => {
                Poll::Ready(result.map(|r| lambda_to_hyper_response(r.into_response())))
            }
        }
    }
}

fn lambda_to_hyper_response(
    r: lambda_http::Response<lambda_http::Body>,
) -> hyper::Response<hyper::Body> {
    let (parts, body) = r.into_parts();

    let body = match body {
        lambda_http::Body::Empty => hyper::Body::empty(),
        lambda_http::Body::Text(s) => hyper::Body::from(s),
        lambda_http::Body::Binary(v) => hyper::Body::from(v),
    };

    Response::from_parts(parts, body)
}

fn hyper_to_lambda_request(_req: hyper::Request<hyper::Body>) -> lambda_http::Request {
    /*
    let (parts, body) = req.into_parts();
    let body = hyper::body::to_bytes(body).await?; // TODO needs to bridge that async in the manual world
    let body = lambda_http::Body::Binary(body.to_vec());

    Request::from_parts(parts, body)
    */
    todo!()
}
