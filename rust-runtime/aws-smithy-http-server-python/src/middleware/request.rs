/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Python-compatible middleware [http::Request] implementation.

use std::collections::HashMap;
use std::str::FromStr;

use aws_smithy_http_server::body::Body;
use http::header::HeaderName;
use http::{HeaderValue, Request};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

/// Python-compatible [Request] object.
///
/// For performance reasons, there is not support yet to pass the body to the Python middleware,
/// as it requires to consume and clone the body, which is a very expensive operation.
///
/// TODO(if customers request for it, we can implemented an opt-in functionality to also pass
/// the body around).
#[pyclass(name = "Request")]
#[pyo3(text_signature = "(request)")]
#[derive(Debug)]
pub struct PyRequest(Option<Request<Body>>);

impl PyRequest {
    /// Create a new Python-compatible [Request] structure from the Rust side.
    pub fn new(request: Request<Body>) -> Self {
        Self(Some(request))
    }

    pub fn take_inner(&mut self) -> Option<Request<Body>> {
        self.0.take()
    }
}

#[pymethods]
impl PyRequest {
    /// Return the HTTP method of this request.
    #[getter]
    fn method(&self) -> PyResult<String> {
        self.0
            .as_ref()
            .map(|req| req.method().to_string())
            .ok_or_else(|| PyRuntimeError::new_err("request is gone"))
    }

    /// Return the URI of this request.
    #[getter]
    fn uri(&self) -> PyResult<String> {
        self.0
            .as_ref()
            .map(|req| req.uri().to_string())
            .ok_or_else(|| PyRuntimeError::new_err("request is gone"))
    }

    /// Return the HTTP version of this request.
    #[getter]
    fn version(&self) -> PyResult<String> {
        self.0
            .as_ref()
            .map(|req| format!("{:?}", req.version()))
            .ok_or_else(|| PyRuntimeError::new_err("request is gone"))
    }

    /// Return the HTTP headers of this request.
    /// TODO(can we use `Py::clone_ref()` to prevent cloning the hashmap?)
    #[getter]
    fn headers(&self) -> PyResult<HashMap<String, String>> {
        self.0
            .as_ref()
            .map(|req| {
                req.headers()
                    .into_iter()
                    .map(|(k, v)| -> (String, String) {
                        let name: String = k.as_str().to_string();
                        let value: String = String::from_utf8_lossy(v.as_bytes()).to_string();
                        (name, value)
                    })
                    .collect()
            })
            .ok_or_else(|| PyRuntimeError::new_err("request is gone"))
    }

    /// Insert a new key/value into this request's headers.
    /// TODO(investigate if using a PyDict can make the experience more idiomatic)
    /// I'd like to be able to do request.headers.get("my-header") and
    /// request.headers["my-header"] = 42 instead of implementing set_header() and get_header()
    /// under pymethods. The same applies to response.
    #[pyo3(text_signature = "($self, key, value)")]
    fn set_header(&mut self, key: &str, value: &str) -> PyResult<()> {
        match self.0.as_mut() {
            Some(ref mut req) => {
                let key = HeaderName::from_str(key)
                    .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
                let value = HeaderValue::from_str(value)
                    .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
                req.headers_mut().insert(key, value);
                Ok(())
            }
            None => return Err(PyRuntimeError::new_err("request is gone")),
        }
    }
}
