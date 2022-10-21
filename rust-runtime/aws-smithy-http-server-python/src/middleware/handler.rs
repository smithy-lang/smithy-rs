/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Execute pure-Python middleware handler.

use aws_smithy_http_server::body::{Body, BoxBody};
use http::{Request, Response};
use pyo3::{exceptions::PyRuntimeError, prelude::*, types::PyFunction};
use pyo3_asyncio::TaskLocals;
use tower::{util::BoxService, BoxError, Service};

use crate::util::func_metadata;

use super::{PyMiddlewareError, PyRequest, PyResponse};

type PyNextInner = BoxService<Request<Body>, Response<BoxBody>, BoxError>;

#[pyo3::pyclass]
struct PyNext(Option<PyNextInner>);

impl PyNext {
    fn new(inner: PyNextInner) -> Self {
        Self(Some(inner))
    }

    fn take_inner(&mut self) -> Option<PyNextInner> {
        self.0.take()
    }
}

#[pyo3::pymethods]
impl PyNext {
    fn __call__<'p>(&'p mut self, py: Python<'p>, py_req: Py<PyRequest>) -> PyResult<&'p PyAny> {
        let req = py_req
            .borrow_mut(py)
            .take_inner()
            .ok_or_else(|| PyRuntimeError::new_err("already called"))?;
        let mut inner = self
            .take_inner()
            .ok_or_else(|| PyRuntimeError::new_err("already called"))?;
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let res = inner
                .call(req)
                .await
                .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
            Ok(Python::with_gil(|py| PyResponse::new(res).into_py(py)))
        })
    }
}

/// A Python middleware handler function representation.
///
/// The Python business logic implementation needs to carry some information
/// to be executed properly like if it is a coroutine.
#[derive(Debug, Clone)]
pub struct PyMiddlewareHandler {
    pub name: String,
    pub func: PyObject,
    pub is_coroutine: bool,
}

impl PyMiddlewareHandler {
    pub fn new(py: Python, func: PyObject) -> PyResult<Self> {
        let func_metadata = func_metadata(py, &func)?;
        Ok(Self {
            name: func_metadata.name,
            func,
            is_coroutine: func_metadata.is_coroutine,
        })
    }

    pub async fn call(
        self,
        req: Request<Body>,
        next: PyNextInner,
        locals: TaskLocals,
    ) -> PyResult<Response<BoxBody>> {
        let py_req = PyRequest::new(req);
        let py_next = PyNext::new(next);

        let handler = self.func;
        let result = if self.is_coroutine {
            pyo3_asyncio::tokio::scope(locals, async move {
                Python::with_gil(|py| {
                    let py_handler: &PyFunction = handler.extract(py)?;
                    let output = py_handler.call1((py_req, py_next))?;
                    pyo3_asyncio::tokio::into_future(output)
                })
            })
            .await?
            .await?
        } else {
            Python::with_gil(|py| {
                let py_handler: &PyFunction = handler.extract(py)?;
                let output = py_handler.call1((py_req, py_next))?;
                Ok::<_, PyErr>(output.into())
            })?
        };

        let response = Python::with_gil(|py| {
            let py_res: Py<PyResponse> = result.extract(py)?;
            let mut py_res = py_res.borrow_mut(py);
            Ok::<_, PyErr>(py_res.take_inner())
        })?;

        response.ok_or_else(|| PyMiddlewareError::ResponseAlreadyGone.into())
    }
}
