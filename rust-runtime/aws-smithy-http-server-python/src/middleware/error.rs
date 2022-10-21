use std::error::Error;
use std::fmt;

use pyo3::{exceptions::PyRuntimeError, PyErr};

#[derive(Debug)]
pub enum PyMiddlewareError {
    ResponseAlreadyGone,
}

impl fmt::Display for PyMiddlewareError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::ResponseAlreadyGone => write!(f, "response is already consumed"),
        }
    }
}

impl Error for PyMiddlewareError {}

impl From<PyMiddlewareError> for PyErr {
    fn from(err: PyMiddlewareError) -> PyErr {
        PyRuntimeError::new_err(err.to_string())
    }
}
