use pyo3::prelude::*;

mod layer;
mod request;
mod response;

pub use self::layer::{PyMiddleware, PyMiddlewareLayer};
pub use self::request::PyRequest;
pub use self::response::PyResponse;

#[pyclass(name = "MiddlewareException", extends = pyo3::exceptions::PyException)]
#[derive(Debug, Clone)]
pub struct PyMiddlewareException {
    #[pyo3(get, set)]
    pub message: String,
    #[pyo3(get, set)]
    pub status_code: u16,
}

#[pymethods]
impl PyMiddlewareException {
    #[new]
    fn newpy(message: String, status_code: u16) -> Self {
        Self {
            message,
            status_code,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PyMiddlewareHandler {
    pub name: String,
    pub func: PyObject,
    pub is_coroutine: bool,
    pub with_body: bool,
}

// Our request handler. This is where we would implement the application logic
// for responding to HTTP requests...
pub async fn execute_middleware(
    request: PyRequest,
    handler: PyMiddlewareHandler,
) -> PyResult<(Option<PyRequest>, Option<PyResponse>)> {
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
    // Catch and record a Python traceback.
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
        Err(e) => {
            let traceback = pyo3::Python::with_gil(|py| match e.traceback(py) {
                Some(t) => t.format().unwrap_or_else(|e| e.to_string()),
                None => "Unknown traceback\n".to_string(),
            });
            tracing::error!("{}{}", traceback, e);
            Err(e)
        }
    })
}
