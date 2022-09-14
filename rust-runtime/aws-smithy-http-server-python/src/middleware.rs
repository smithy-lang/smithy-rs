//! Authorize requests using the [`Authorization`] header asynchronously.
//!
//! [`Authorization`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Authorization
//!
//! # Example
//!
//! ```
//! use tower_http::auth::{AsyncRequireAuthorizationLayer, AsyncAuthorizeRequest};
//! use hyper::{Request, Response, Body, Error};
//! use http::{StatusCode, header::AUTHORIZATION};
//! use tower::{Service, ServiceExt, ServiceBuilder, service_fn};
//! use futures_util::future::BoxFuture;
//!
//! #[derive(Clone, Copy)]
//! struct MyAuth;
//!
//! impl<B> AsyncAuthorizeRequest<B> for MyAuth
//! where
//!     B: Send + Sync + 'static,
//! {
//!     type RequestBody = B;
//!     type ResponseBody = Body;
//!     type Future = BoxFuture<'static, Result<Request<B>, Response<Self::ResponseBody>>>;
//!
//!     fn authorize(&mut self, mut request: Request<B>) -> Self::Future {
//!         Box::pin(async {
//!             if let Some(user_id) = check_auth(&request).await {
//!                 // Set `user_id` as a request extension so it can be accessed by other
//!                 // services down the stack.
//!                 request.extensions_mut().insert(user_id);
//!
//!                 Ok(request)
//!             } else {
//!                 let unauthorized_response = Response::builder()
//!                     .status(StatusCode::UNAUTHORIZED)
//!                     .body(Body::empty())
//!                     .unwrap();
//!
//!                 Err(unauthorized_response)
//!             }
//!         })
//!     }
//! }
//!
//! async fn check_auth<B>(request: &Request<B>) -> Option<UserId> {
//!     // ...
//!     # None
//! }
//!
//! #[derive(Debug)]
//! struct UserId(String);
//!
//! async fn handle(request: Request<Body>) -> Result<Response<Body>, Error> {
//!     // Access the `UserId` that was set in `on_authorized`. If `handle` gets called the
//!     // request was authorized and `UserId` will be present.
//!     let user_id = request
//!         .extensions()
//!         .get::<UserId>()
//!         .expect("UserId will be there if request was authorized");
//!
//!     println!("request from {:?}", user_id);
//!
//!     Ok(Response::new(Body::empty()))
//! }
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let service = ServiceBuilder::new()
//!     // Authorize requests using `MyAuth`
//!     .layer(AsyncRequireAuthorizationLayer::new(MyAuth))
//!     .service_fn(handle);
//! # Ok(())
//! # }
//! ```
//!
//! Or using a closure:
//!
//! ```
//! use tower_http::auth::{AsyncRequireAuthorizationLayer, AsyncAuthorizeRequest};
//! use hyper::{Request, Response, Body, Error};
//! use http::StatusCode;
//! use tower::{Service, ServiceExt, ServiceBuilder};
//! use futures_util::future::BoxFuture;
//!
//! async fn check_auth<B>(request: &Request<B>) -> Option<UserId> {
//!     // ...
//!     # None
//! }
//!
//! #[derive(Debug)]
//! struct UserId(String);
//!
//! async fn handle(request: Request<Body>) -> Result<Response<Body>, Error> {
//!     # todo!();
//!     // ...
//! }
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let service = ServiceBuilder::new()
//!     .layer(AsyncRequireAuthorizationLayer::new(|request: Request<Body>| async move {
//!         if let Some(user_id) = check_auth(&request).await {
//!             Ok(request)
//!         } else {
//!             let unauthorized_response = Response::builder()
//!                 .status(StatusCode::UNAUTHORIZED)
//!                 .body(Body::empty())
//!                 .unwrap();
//!
//!             Err(unauthorized_response)
//!         }
//!     }))
//!     .service_fn(handle);
//! # Ok(())
//! # }
//! ```

use aws_smithy_http_server::body::{boxed, BoxBody};
use bytes::Bytes;
use futures::future::BoxFuture;
use futures_core::ready;
use http::{Request, Response};
use hyper::Body;
use pin_project_lite::pin_project;
use pyo3::{pyclass, IntoPy, PyAny, PyErr, PyObject, PyResult};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll}, error::Error,
};
use tower::{Layer, Service};

#[pyclass(name = "Request")]
#[derive(Debug)]
pub struct PyRequest(Request<Bytes>);

impl PyRequest {
    pub fn new<B>(request: &Request<B>) -> Self {
        let mut self_ = Request::builder()
            .uri(request.uri())
            .method(request.method())
            .body(Bytes::new())
            .unwrap();
        let headers = self_.headers_mut();
        *headers = request.headers().clone();
        Self(self_)
    }

    pub fn new_with_body<B>(request: &Request<B>) -> Self {
        let mut self_ = Request::builder()
            .uri(request.uri())
            .method(request.method())
            .body(Bytes::new())
            .unwrap();
        let headers = self_.headers_mut();
        *headers = request.headers().clone();
        Self(self_)
    }
}

impl Clone for PyRequest {
    fn clone(&self) -> Self {
        let mut request = Request::builder()
            .uri(self.0.uri())
            .method(self.0.method())
            .body(self.0.body().clone())
            .unwrap();
        let headers = request.headers_mut();
        *headers = self.0.headers().clone();
        Self(request)
    }
}

/// A Python handler function representation.
///
/// The Python business logic implementation needs to carry some information
/// to be executed properly like the size of its arguments and if it is
/// a coroutine.
#[derive(Debug, Clone)]
pub struct PyMiddlewareHandler {
    pub name: String,
    pub func: PyObject,
    pub is_coroutine: bool,
    pub with_body: bool,
}

#[derive(Debug, Clone)]
pub struct PyMiddlewareHandlers(pub Vec<PyMiddlewareHandler>);

// Our request handler. This is where we would implement the application logic
// for responding to HTTP requests...
pub async fn middleware_wrapper(
    request: PyRequest,
    handler: PyMiddlewareHandler,
) -> PyResult<()> {
    let result = if handler.is_coroutine {
        tracing::debug!("Executing Python handler coroutine `stream_pokemon_radio_operation()`");
        let result = pyo3::Python::with_gil(|py| {
            let pyhandler: &pyo3::types::PyFunction = handler.func.extract(py)?;
            let coroutine = pyhandler.call1((request,))?;
            pyo3_asyncio::tokio::into_future(coroutine)
        })?;
        result
            .await
            .map(|_| ())
    } else {
        tracing::debug!("Executing Python handler function `stream_pokemon_radio_operation()`");
        tokio::task::block_in_place(move || {
            pyo3::Python::with_gil(|py| {
                let pyhandler: &pyo3::types::PyFunction = handler.func.extract(py)?;
                pyhandler.call1((request,))?;
                Ok(())
            })
        })
    };
    // Catch and record a Python traceback.
    result.map_err(|e| {
        let traceback = pyo3::Python::with_gil(|py| match e.traceback(py) {
            Some(t) => t.format().unwrap_or_else(|e| e.to_string()),
            None => "Unknown traceback\n".to_string(),
        });
        tracing::error!("{}{}", traceback, e);
        e
    })?;
    Ok(())
}

impl<B> PyMiddleware<B> for PyMiddlewareHandlers
where
    B: Send + Sync + 'static,
{
    type RequestBody = B;
    type ResponseBody = BoxBody;
    type Future = BoxFuture<'static, Result<Request<B>, Response<Self::ResponseBody>>>;

    fn run(&mut self, request: Request<B>) -> Self::Future {
        let handlers = self.0.clone();
        Box::pin(async move {
            for handler in handlers {
                let pyrequest = if handler.with_body {
                    PyRequest::new_with_body(&request)
                } else {
                    PyRequest::new(&request)
                };
                middleware_wrapper(pyrequest, handler).await.map_err(|e| into_response(e))?;
            }
            Ok(request)
        })
    }
}

fn into_response<T: Error>(error: T) -> Response<BoxBody> {
    Response::builder().status(500).body(boxed(error.to_string())).unwrap()

}

/// Layer that applies [`AsyncRequireAuthorization`] which authorizes all requests using the
/// [`Authorization`] header.
///
/// See the [module docs](crate::auth::async_require_authorization) for an example.
///
/// [`Authorization`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Authorization
#[derive(Debug, Clone)]
pub struct PyMiddlewareLayer<T> {
    handler: T,
}

impl<T> PyMiddlewareLayer<T> {
    /// Authorize requests using a custom scheme.
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

/// Middleware that authorizes all requests using the [`Authorization`] header.
///
/// See the [module docs](crate::auth::async_require_authorization) for an example.
///
/// [`Authorization`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Authorization
#[derive(Clone, Debug)]
pub struct PyMiddlewareService<S, T> {
    inner: S,
    handler: T,
}

impl<S, T> PyMiddlewareService<S, T> {
    /// Authorize requests using a custom scheme.
    ///
    /// The `Authorization` header is required to have the value provided.
    pub fn new(inner: S, handler: T) -> PyMiddlewareService<S, T> {
        Self { inner, handler }
    }

    /// Returns a new [`Layer`] that wraps services with an [`AsyncRequireAuthorizationLayer`]
    /// middleware.
    ///
    /// [`Layer`]: tower_layer::Layer
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
    /// Response future for [`AsyncRequireAuthorization`].
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

/// Trait for authorizing requests.
pub trait PyMiddleware<B> {
    type RequestBody;
    type ResponseBody;
    /// The Future type returned by `authorize`
    type Future: Future<Output = Result<Request<Self::RequestBody>, Response<Self::ResponseBody>>>;

    /// Authorize the request.
    ///
    /// If the future resolves to `Ok(request)` then the request is allowed through, otherwise not.
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

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    use futures_util::future::BoxFuture;
    use http::{header, StatusCode};
    use hyper::Body;
    use tower::{BoxError, ServiceBuilder, ServiceExt};

    #[derive(Clone, Copy)]
    struct MyAuth;

    impl<B> PyMiddleware<B> for MyAuth
    where
        B: Send + 'static,
    {
        type RequestBody = B;
        type ResponseBody = Body;
        type Future = BoxFuture<'static, Result<Request<B>, Response<Self::ResponseBody>>>;

        fn authorize(&mut self, mut request: Request<B>) -> Self::Future {
            Box::pin(async move {
                let authorized = request
                    .headers()
                    .get(header::AUTHORIZATION)
                    .and_then(|it| it.to_str().ok())
                    .and_then(|it| it.strip_prefix("Bearer "))
                    .map(|it| it == "69420")
                    .unwrap_or(false);

                if authorized {
                    let user_id = UserId("6969".to_owned());
                    request.extensions_mut().insert(user_id);
                    Ok(request)
                } else {
                    Err(Response::builder()
                        .status(StatusCode::UNAUTHORIZED)
                        .body(Body::empty())
                        .unwrap())
                }
            })
        }
    }

    #[derive(Debug)]
    struct UserId(String);

    #[tokio::test]
    async fn require_async_auth_works() {
        let mut service = ServiceBuilder::new()
            .layer(PyMiddlewareLayer::new(MyAuth))
            .service_fn(echo);

        let request = Request::get("/")
            .header(header::AUTHORIZATION, "Bearer 69420")
            .body(Body::empty())
            .unwrap();

        let res = service.ready().await.unwrap().call(request).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn require_async_auth_401() {
        let mut service = ServiceBuilder::new()
            .layer(PyMiddlewareLayer::new(MyAuth))
            .service_fn(echo);

        let request = Request::get("/")
            .header(header::AUTHORIZATION, "Bearer deez")
            .body(Body::empty())
            .unwrap();

        let res = service.ready().await.unwrap().call(request).await.unwrap();

        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    async fn echo(req: Request<Body>) -> Result<Response<Body>, BoxError> {
        Ok(Response::new(req.into_body()))
    }
}
