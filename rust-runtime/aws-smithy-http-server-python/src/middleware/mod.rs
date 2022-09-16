use pyo3::prelude::*;

mod layer;
mod request;

pub use self::layer::{PyMiddleware, PyMiddlewareLayer};
pub use self::request::PyRequest;

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
pub async fn py_middleware_wrapper(
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
        result.await.map(|_| ())
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
