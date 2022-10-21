use pyo3::{exceptions::PyRuntimeError, PyErr};
use thiserror::Error;

/// Possible middleware errors that might arise.
#[derive(Error, Debug)]
pub enum PyMiddlewareError {
    /// Returned when `next` is called multiple times.
    #[error("next already called")]
    NextAlreadyCalled,
    /// Returned when request is accessed after `next` is called.
    #[error("request is gone")]
    RequestGone,
    /// Returned when response is called after it is returned.
    #[error("response is gone")]
    ResponseGone,
}

impl From<PyMiddlewareError> for PyErr {
    fn from(err: PyMiddlewareError) -> PyErr {
        PyRuntimeError::new_err(err.to_string())
    }
}
