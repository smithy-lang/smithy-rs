/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Execute pure-Python middleware handler.

use aws_smithy_http_server::body::{Body, BoxBody};
use http::{Request, Response};
use parking_lot::Mutex;
use pyo3::{exceptions::PyRuntimeError, prelude::*, types::PyFunction};
use pyo3_async_runtimes::TaskLocals;
use tower::{util::BoxService, BoxError, Service};

use crate::util::func_metadata;

use super::{PyMiddlewareError, PyRequest, PyResponse};

// PyNextInner represents the inner service Tower layer applied to.
type PyNextInner = BoxService<Request<Body>, Response<BoxBody>, BoxError>;

trait CallBackTrait: Send {
    fn call(&self, py: Python, request: Request<Body>) -> PyResult<PyObject>;
}

// PyNext wraps inner Tower service and makes it callable from Python.
#[pyo3::pyclass]
struct PyNext(Mutex<Option<PyNextInner>>);

impl PyNext {
    fn new(inner: PyNextInner) -> Self {
        Self(Some(inner))
    }

    // Consumes self by taking the inner Tower service.
    // This method would have been `into_inner(self) -> PyNextInner`
    // but we can't do that because we are crossing Python boundary.
    fn take_inner(&mut self) -> Option<PyNextInner> {
        self.0.take()
    }
}

#[pyo3::pymethods]
impl PyNext {
    // Calls the inner Tower service with the `Request` that is passed from Python.
    // It returns a coroutine to be awaited on the Python side to complete the call.
    // Note that it takes wrapped objects from both `PyRequest` and `PyNext`,
    // so after calling `next`, consumer can't access to the `Request` or
    // can't call the `next` again, this basically emulates consuming `self` and `Request`,
    // but since we are crossing the Python boundary we can't express it in natural Rust terms.
    //
    // Naming the method `__call__` allows `next` to be called like `next(...)`.
    fn __call__<'p>(&'p mut self, py: Python<'p>, py_req: Py<PyRequest>) -> PyResult<Bound<PyAny>> {
        let req = py_req
            .borrow_mut(py)
            .take_inner()
            .ok_or(PyMiddlewareError::RequestGone)?;
        let mut inner = self
            .take_inner()
            .ok_or(PyMiddlewareError::NextAlreadyCalled)?;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let res = inner
                .call(req)
                .await
                .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
            Ok::<_, PyErr>(Python::with_gil(|py| PyResponse::new(res).into_py(py)))
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

    // Calls pure-Python middleware handler with given `Request` and the next Tower service
    // and returns the `Response` that returned from the pure-Python handler.
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
            pyo3_async_runtimes::tokio::scope(locals, async move {
                Python::with_gil(|py| {
                    let py_handler: &Bound<'_, PyFunction> = handler.downcast_bound(py)?;
                    let output = py_handler.call1((py_req, py_next))?;
                    pyo3_async_runtimes::tokio::into_future(output)
                })?
                .await
            })
            .await?
        } else {
            Python::with_gil(|py| {
                let py_handler: &Bound<'_, PyFunction> = handler.downcast_bound(py)?;
                let output = py_handler.call1((py_req, py_next))?;
                Ok::<_, PyErr>(output.into())
            })?
        };

        let response = Python::with_gil(|py| {
            let py_res: Py<PyResponse> = result.extract(py)?;
            let mut py_res = py_res.borrow_mut(py);
            Ok::<_, PyErr>(py_res.take_inner())
        })?;

        response.ok_or_else(|| PyMiddlewareError::ResponseGone.into())
    }
}
