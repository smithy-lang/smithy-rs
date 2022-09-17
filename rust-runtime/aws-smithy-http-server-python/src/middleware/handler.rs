/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Execute Python middleware handlers.
use aws_smithy_http_server::body::Body;
use http::Request;
use pyo3::prelude::*;

use aws_smithy_http_server::protocols::Protocol;
use pyo3_asyncio::TaskLocals;

use crate::{PyMiddlewareException, PyRequest, PyResponse};

use super::PyFuture;

#[derive(Debug, Clone)]
pub struct PyMiddlewareHandler {
    pub name: String,
    pub func: PyObject,
    pub is_coroutine: bool,
}

#[derive(Debug, Clone, Default)]
pub struct PyMiddlewares(pub Vec<PyMiddlewareHandler>);

impl PyMiddlewares {
    pub fn new(handlers: Vec<PyMiddlewareHandler>) -> Self {
        Self(handlers)
    }

    pub fn push(&mut self, handler: PyMiddlewareHandler) {
        self.0.push(handler);
    }

    // Our request handler. This is where we would implement the application logic
    // for responding to HTTP requests...
    async fn execute_middleware(
        request: PyRequest,
        handler: PyMiddlewareHandler,
    ) -> Result<(Option<PyRequest>, Option<PyResponse>), PyMiddlewareException> {
        let handle: PyResult<pyo3::Py<pyo3::PyAny>> = if handler.is_coroutine {
            tracing::debug!("Executing Python middleware coroutine `{}`", handler.name);
            let result = pyo3::Python::with_gil(|py| {
                let pyhandler: &pyo3::types::PyFunction = handler.func.extract(py)?;
                let coroutine = pyhandler.call1((request,))?;
                pyo3_asyncio::tokio::into_future(coroutine)
            })?;
            let output = result.await?;
            Ok(output)
        } else {
            tracing::debug!("Executing Python middleware function `{}`", handler.name);
            pyo3::Python::with_gil(|py| {
                let pyhandler: &pyo3::types::PyFunction = handler.func.extract(py)?;
                let output = pyhandler.call1((request,))?;
                Ok(output.into_py(py))
            })
        };
        Python::with_gil(|py| match handle {
            Ok(result) => {
                if let Ok(request) = result.extract::<PyRequest>(py) {
                    return Ok((Some(request), None));
                }
                if let Ok(response) = result.extract::<PyResponse>(py) {
                    return Ok((None, Some(response)));
                }
                Ok((None, None))
            }
            Err(e) => pyo3::Python::with_gil(|py| {
                let traceback = match e.traceback(py) {
                    Some(t) => t.format().unwrap_or_else(|e| e.to_string()),
                    None => "Unknown traceback\n".to_string(),
                };
                tracing::error!("{}{}", traceback, e);
                let variant = e.value(py);
                if let Ok(v) = variant.extract::<PyMiddlewareException>() {
                    Err(v)
                } else {
                    Err(e.into())
                }
            }),
        })
    }

    pub fn run(
        &mut self,
        mut request: Request<Body>,
        protocol: Protocol,
        locals: TaskLocals,
    ) -> PyFuture {
        let handlers = self.0.clone();
        // Run all Python handlers in a loop.
        Box::pin(async move {
            tracing::debug!("Executing Python middleware stack");
            for handler in handlers {
                let name = handler.name.clone();
                let pyrequest = PyRequest::new(&request);
                let loop_locals = locals.clone();
                let result = pyo3_asyncio::tokio::scope(
                    loop_locals,
                    Self::execute_middleware(pyrequest, handler),
                )
                .await;
                match result {
                    Ok((pyrequest, pyresponse)) => {
                        if let Some(pyrequest) = pyrequest {
                            if let Ok(headers) = (&pyrequest.headers).try_into() {
                                tracing::debug!("Middleware `{name}` returned an HTTP request, override headers with middleware's one");
                                *request.headers_mut() = headers;
                            }
                        }
                        if let Some(pyresponse) = pyresponse {
                            tracing::debug!(
                            "Middleware `{name}` returned a HTTP response, exit middleware loop"
                        );
                            return Err(pyresponse.into());
                        }
                    }
                    Err(e) => {
                        tracing::debug!(
                            "Middleware `{name}` returned an error, exit middleware loop"
                        );
                        return Err(e.into_response(protocol));
                    }
                }
            }
            tracing::debug!("Returning original request to operation handler");
            Ok(request)
        })
    }
}

#[cfg(test)]
mod tests {
    use http::HeaderValue;
    use hyper::body::to_bytes;
    use pretty_assertions::assert_eq;

    use super::*;

    #[tokio::test]
    async fn middleware_chain_keeps_headers_changes() -> PyResult<()> {
        let locals = crate::tests::initialize();
        let mut middlewares = PyMiddlewares(vec![]);

        Python::with_gil(|py| {
            let middleware = PyModule::new(py, "middleware").unwrap();
            middleware.add_class::<PyRequest>().unwrap();
            middleware.add_class::<PyMiddlewareException>().unwrap();
            let pycode = r#"
def first_middleware(request: Request):
    request.set_header("x-amzn-answer", "42")
    return request

async def second_middleware(request: Request):
    if request.get_header("x-amzn-answer") != "42":
        raise MiddlewareException("wrong answer", 401)
"#;
            py.run(pycode, Some(middleware.dict()), None)?;
            let all = middleware.index()?;
            let first_middleware = PyMiddlewareHandler {
                func: middleware.getattr("first_middleware")?.into_py(py),
                is_coroutine: false,
                name: "first".to_string(),
            };
            all.append("first_middleware")?;
            middlewares.push(first_middleware);
            let second_middleware = PyMiddlewareHandler {
                func: middleware.getattr("second_middleware")?.into_py(py),
                is_coroutine: false,
                name: "second".to_string(),
            };
            all.append("second_middleware")?;
            middlewares.push(second_middleware);
            Ok::<(), PyErr>(())
        })?;

        let result = middlewares
            .run(
                Request::builder().body(Body::from("")).unwrap(),
                Protocol::RestJson1,
                locals,
            )
            .await
            .unwrap();
        assert_eq!(
            result.headers().get("x-amzn-answer"),
            Some(&HeaderValue::from_static("42"))
        );
        Ok(())
    }

    #[tokio::test]
    async fn middleware_return_response() -> PyResult<()> {
        let locals = crate::tests::initialize();
        let mut middlewares = PyMiddlewares(vec![]);

        Python::with_gil(|py| {
            let middleware = PyModule::new(py, "middleware").unwrap();
            middleware.add_class::<PyRequest>().unwrap();
            middleware.add_class::<PyResponse>().unwrap();
            let pycode = r#"
def middleware(request: Request):
    return Response(200, {}, b"something")"#;
            py.run(pycode, Some(middleware.dict()), None)?;
            let all = middleware.index()?;
            let middleware = PyMiddlewareHandler {
                func: middleware.getattr("middleware")?.into_py(py),
                is_coroutine: false,
                name: "middleware".to_string(),
            };
            all.append("middleware")?;
            middlewares.push(middleware);
            Ok::<(), PyErr>(())
        })?;

        let result = middlewares
            .run(
                Request::builder().body(Body::from("")).unwrap(),
                Protocol::RestJson1,
                locals,
            )
            .await
            .unwrap_err();
        assert_eq!(result.status(), 200);
        let body = to_bytes(result.into_body()).await.unwrap();
        assert_eq!(body, "something".as_bytes());
        Ok(())
    }

    #[tokio::test]
    async fn middleware_raise_middleware_exception() -> PyResult<()> {
        let locals = crate::tests::initialize();
        let mut middlewares = PyMiddlewares(vec![]);

        Python::with_gil(|py| {
            let middleware = PyModule::new(py, "middleware").unwrap();
            middleware.add_class::<PyRequest>().unwrap();
            middleware.add_class::<PyMiddlewareException>().unwrap();
            let pycode = r#"
def middleware(request: Request):
    raise MiddlewareException("error", 503)"#;
            py.run(pycode, Some(middleware.dict()), None)?;
            let all = middleware.index()?;
            let middleware = PyMiddlewareHandler {
                func: middleware.getattr("middleware")?.into_py(py),
                is_coroutine: false,
                name: "middleware".to_string(),
            };
            all.append("middleware")?;
            middlewares.push(middleware);
            Ok::<(), PyErr>(())
        })?;

        let result = middlewares
            .run(
                Request::builder().body(Body::from("")).unwrap(),
                Protocol::RestJson1,
                locals,
            )
            .await
            .unwrap_err();
        assert_eq!(result.status(), 503);
        assert_eq!(
            result.headers().get("X-Amzn-Errortype"),
            Some(&HeaderValue::from_static("MiddlewareException"))
        );
        let body = to_bytes(result.into_body()).await.unwrap();
        assert_eq!(body, r#"{"message":"error"}"#.as_bytes());
        Ok(())
    }

    #[tokio::test]
    async fn middleware_raise_python_exception() -> PyResult<()> {
        let locals = crate::tests::initialize();
        let mut middlewares = PyMiddlewares(vec![]);

        Python::with_gil(|py| {
            let middleware = PyModule::from_code(
                py,
                r#"
def middleware(request):
    raise ValueError("error")"#,
                "",
                "",
            )?;
            let middleware = PyMiddlewareHandler {
                func: middleware.getattr("middleware")?.into_py(py),
                is_coroutine: false,
                name: "middleware".to_string(),
            };
            middlewares.push(middleware);
            Ok::<(), PyErr>(())
        })?;

        let result = middlewares
            .run(
                Request::builder().body(Body::from("")).unwrap(),
                Protocol::RestJson1,
                locals,
            )
            .await
            .unwrap_err();
        assert_eq!(result.status(), 500);
        assert_eq!(
            result.headers().get("X-Amzn-Errortype"),
            Some(&HeaderValue::from_static("MiddlewareException"))
        );
        let body = to_bytes(result.into_body()).await.unwrap();
        assert_eq!(body, r#"{"message":"ValueError: error"}"#.as_bytes());
        Ok(())
    }
}
