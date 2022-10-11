/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Tower layer implementation of Python middleware handling.
use std::{
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use aws_smithy_http_server::{
    body::{Body, BoxBody},
    response::IntoResponse,
};
use futures::{ready, Future};
use http::{Request, Response};
use pin_project_lite::pin_project;
use pyo3_asyncio::TaskLocals;
use tower::{Layer, Service};

use crate::{middleware::PyFuture, PyMiddlewareException, PyMiddlewares};

/// Tower [Layer] implementation of Python middleware handling.
///
/// Middleware stored in the `handlers` attribute will be executed, in order,
/// inside an async Tower middleware.
#[derive(Debug, Clone)]
pub struct PyMiddlewareLayer<P> {
    handlers: PyMiddlewares,
    locals: TaskLocals,
    _protocol: PhantomData<P>,
}

impl<P> PyMiddlewareLayer<P> {
    pub fn new(handlers: PyMiddlewares, locals: TaskLocals) -> Self {
        Self {
            handlers,
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
        PyMiddlewareService::new(inner, self.handlers.clone(), self.locals.clone())
    }
}

// Tower [Service] wrapping the Python middleware [Layer].
#[derive(Clone, Debug)]
pub struct PyMiddlewareService<S> {
    inner: S,
    handlers: PyMiddlewares,
    locals: TaskLocals,
}

impl<S> PyMiddlewareService<S> {
    pub fn new(inner: S, handlers: PyMiddlewares, locals: TaskLocals) -> PyMiddlewareService<S> {
        Self {
            inner,
            handlers,
            locals,
        }
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
        // TODO(Should we make this clone less expensive by wrapping inner in a Arc?)
        let clone = self.inner.clone();
        // See https://docs.rs/tower/latest/tower/trait.Service.html#be-careful-when-cloning-inner-services
        let inner = std::mem::replace(&mut self.inner, clone);
        let run = self.handlers.run(req, self.locals.clone());

        ResponseFuture {
            middleware: State::Running { run },
            service: inner,
        }
    }
}

pin_project! {
    /// Response future handling the state transition between a running and a done future.
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
    /// Representation of the result of the middleware execution.
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
                // Run the handler and store the future inside the inner state.
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
                // Execute the future returned by the layer.
                StateProj::Done { fut } => return fut.poll(cx),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;

    use aws_smithy_http_server::body::to_boxed;
    use aws_smithy_http_server::proto::rest_json_1::AwsRestJson1;
    use pyo3::prelude::*;
    use tower::{Service, ServiceBuilder, ServiceExt};

    use crate::middleware::PyMiddlewareHandler;
    use crate::{PyMiddlewareException, PyMiddlewareType, PyRequest};

    async fn echo(req: Request<Body>) -> Result<Response<BoxBody>, Box<dyn Error + Send + Sync>> {
        Ok(Response::new(to_boxed(req.into_body())))
    }

    #[tokio::test]
    async fn request_middlewares_are_chained_inside_layer() -> PyResult<()> {
        let locals = crate::tests::initialize();
        let mut middlewares = PyMiddlewares::new::<AwsRestJson1>(vec![]);

        Python::with_gil(|py| {
            let middleware = PyModule::new(py, "middleware").unwrap();
            middleware.add_class::<PyRequest>().unwrap();
            middleware.add_class::<PyMiddlewareException>().unwrap();
            let pycode = r#"
def first_middleware(request: Request):
    request.set_header("x-amzn-answer", "42")
    return request

def second_middleware(request: Request):
    if request.get_header("x-amzn-answer") != "42":
        raise MiddlewareException("wrong answer", 401)
"#;
            py.run(pycode, Some(middleware.dict()), None)?;
            let all = middleware.index()?;
            let first_middleware = PyMiddlewareHandler {
                func: middleware.getattr("first_middleware")?.into_py(py),
                is_coroutine: false,
                name: "first".to_string(),
                _type: PyMiddlewareType::Request,
            };
            all.append("first_middleware")?;
            middlewares.push(first_middleware);
            let second_middleware = PyMiddlewareHandler {
                func: middleware.getattr("second_middleware")?.into_py(py),
                is_coroutine: false,
                name: "second".to_string(),
                _type: PyMiddlewareType::Request,
            };
            all.append("second_middleware")?;
            middlewares.push(second_middleware);
            Ok::<(), PyErr>(())
        })?;

        let mut service = ServiceBuilder::new()
            .layer(PyMiddlewareLayer::<AwsRestJson1>::new(middlewares, locals))
            .service_fn(echo);

        let request = Request::get("/").body(Body::empty()).unwrap();

        let res = service.ready().await.unwrap().call(request).await.unwrap();

        assert_eq!(res.status(), 200);
        Ok(())
    }
}
