/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! A [tower::Layer] for injecting and populating [PyContext].

use std::task::{Context, Poll};

use http::{Request, Response};
use tower::{Layer, Service};

use super::PyContext;

/// AddPyContextLayer is a [tower::Layer] that populates given [PyContext] from the [Request]
/// and injects [PyContext] to the [Request] as an extension.
pub struct AddPyContextLayer {
    ctx: PyContext,
}

impl AddPyContextLayer {
    pub fn new(ctx: PyContext) -> Self {
        Self { ctx }
    }
}

impl<S> Layer<S> for AddPyContextLayer {
    type Service = AddPyContextService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AddPyContextService {
            inner,
            ctx: self.ctx.clone(),
        }
    }
}

#[derive(Clone)]
pub struct AddPyContextService<S> {
    inner: S,
    ctx: PyContext,
}

impl<ResBody, ReqBody, S> Service<Request<ReqBody>> for AddPyContextService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        self.ctx.populate_from_extensions(req.extensions());
        req.extensions_mut().insert(self.ctx.clone());
        self.inner.call(req)
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use http::{Request, Response};
    use hyper::Body;
    use pyo3::prelude::*;
    use pyo3::types::IntoPyDict;
    use tower::{service_fn, ServiceBuilder, ServiceExt};

    use crate::context::testing::{get_context, lambda_ctx};

    use super::*;

    #[tokio::test]
    async fn populates_lambda_context() {
        pyo3::prepare_freethreaded_python();

        let ctx = get_context(
            r#"
class Context:
    counter: int = 42
    lambda_ctx: typing.Optional[LambdaContext] = None

ctx = Context()
    "#,
        );

        let svc = ServiceBuilder::new()
            .layer(AddPyContextLayer::new(ctx))
            .service(service_fn(|req: Request<Body>| async move {
                let ctx = req.extensions().get::<PyContext>().unwrap();
                let (req_id, counter) = Python::with_gil(|py| {
                    let locals = [("ctx", ctx)]
                        .into_py_dict(py)
                        .expect("could not convert context to dictionary");
                    py.run(
                        cr#"
req_id = ctx.lambda_ctx.request_id
ctx.counter += 1
counter = ctx.counter
    "#,
                        None,
                        Some(&locals),
                    )
                    .unwrap();

                    (
                        locals
                            .get_item("req_id")
                            .expect("Python exception occurred during dictionary lookup")
                            .unwrap()
                            .to_string(),
                        locals
                            .get_item("counter")
                            .expect("Python exception occurred during dictionary lookup")
                            .unwrap()
                            .to_string(),
                    )
                });
                Ok::<_, Infallible>(Response::new((req_id, counter)))
            }));

        let mut req = Request::new(Body::empty());
        req.extensions_mut().insert(lambda_ctx("my-req-id", "178"));

        let res = svc.oneshot(req).await.unwrap().into_body();

        assert_eq!(("my-req-id".to_string(), "43".to_string()), res);
    }
}
