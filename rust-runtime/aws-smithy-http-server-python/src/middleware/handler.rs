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
pub struct PyMiddlewares(Vec<PyMiddlewareHandler>);

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
