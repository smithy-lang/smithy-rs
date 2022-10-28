/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use pyo3::{exceptions::PyRuntimeError, PyErr};
use thiserror::Error;

/// Possible middleware errors that might arise.
#[derive(Error, Debug)]
pub enum PyMiddlewareError {
    #[error("`next` is called multiple times")]
    NextAlreadyCalled,
    #[error("request is accessed after `next` is called")]
    RequestGone,
    #[error("response is called after it is returned")]
    ResponseGone,
}

impl From<PyMiddlewareError> for PyErr {
    fn from(err: PyMiddlewareError) -> PyErr {
        PyRuntimeError::new_err(err.to_string())
    }
}
