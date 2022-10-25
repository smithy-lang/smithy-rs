/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Python-compatible middleware [http::Response] implementation.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use aws_smithy_http_server::body::{to_boxed, BoxBody};
use http::{header::HeaderName, response::Parts, HeaderValue, Response};
use pyo3::{exceptions::PyRuntimeError, prelude::*};
use tokio::sync::Mutex;

use super::PyMiddlewareError;

/// Python-compatible [Response] object.
#[pyclass(name = "Response")]
#[pyo3(text_signature = "(status, headers, body)")]
pub struct PyResponse {
    parts: Option<Parts>,
    body: Arc<Mutex<Option<BoxBody>>>,
}

impl PyResponse {
    /// Create a new Python-compatible [Response] structure from the Rust side.
    pub fn new(response: Response<BoxBody>) -> Self {
        let (parts, body) = response.into_parts();
        Self {
            parts: Some(parts),
            body: Arc::new(Mutex::new(Some(body))),
        }
    }

    pub fn take_inner(&mut self) -> Option<Response<BoxBody>> {
        let parts = self.parts.take()?;
        let body = std::mem::replace(&mut self.body, Arc::new(Mutex::new(None)));
        let body = Arc::try_unwrap(body).ok()?;
        let body = body.into_inner().take()?;
        Some(Response::from_parts(parts, body))
    }
}

#[pymethods]
impl PyResponse {
    /// Python-compatible [Response] object from the Python side.
    #[new]
    fn newpy(
        status: u16,
        headers: Option<HashMap<String, String>>,
        body: Option<Vec<u8>>,
    ) -> PyResult<Self> {
        let mut builder = Response::builder().status(status);

        if let Some(headers) = headers {
            for (k, v) in headers {
                builder = builder.header(k, v);
            }
        }

        let response = builder
            .body(body.map(to_boxed).unwrap_or_default())
            .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;

        Ok(Self::new(response))
    }

    /// Return the HTTP status of this response.
    #[getter]
    fn status(&self) -> PyResult<u16> {
        self.parts
            .as_ref()
            .map(|parts| parts.status.as_u16())
            .ok_or_else(|| PyMiddlewareError::ResponseGone.into())
    }

    /// Return the HTTP version of this response.
    #[getter]
    fn version(&self) -> PyResult<String> {
        self.parts
            .as_ref()
            .map(|parts| format!("{:?}", parts.version))
            .ok_or_else(|| PyMiddlewareError::ResponseGone.into())
    }

    /// Return the HTTP headers of this response.
    /// TODO(can we use `Py::clone_ref()` to prevent cloning the hashmap?)
    #[getter]
    fn headers(&self) -> PyResult<HashMap<String, String>> {
        self.parts
            .as_ref()
            .map(|parts| {
                parts
                    .headers
                    .iter()
                    .map(|(k, v)| -> (String, String) {
                        let name: String = k.to_string();
                        let value: String = String::from_utf8_lossy(v.as_bytes()).to_string();
                        (name, value)
                    })
                    .collect()
            })
            .ok_or_else(|| PyMiddlewareError::ResponseGone.into())
    }

    /// Insert a new key/value into this response's headers.
    /// TODO(investigate if using a PyDict can make the experience more idiomatic)
    /// I'd like to be able to do response.headers.get("my-header") and
    /// response.headers["my-header"] = 42 instead of implementing set_header() and get_header()
    /// under pymethods. The same applies to request.
    #[pyo3(text_signature = "($self, key, value)")]
    fn set_header(&mut self, key: &str, value: &str) -> PyResult<()> {
        match self.parts.as_mut() {
            Some(parts) => {
                let key = HeaderName::from_str(key)
                    .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
                let value = HeaderValue::from_str(value)
                    .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
                parts.headers.insert(key, value);
                Ok(())
            }
            None => Err(PyMiddlewareError::ResponseGone.into()),
        }
    }

    /// Return the HTTP body of this response.
    /// Note that this is a costly operation because the whole response body is cloned.
    #[getter]
    fn body<'p>(&self, py: Python<'p>) -> PyResult<&'p PyAny> {
        let body = self.body.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let body = {
                let mut body_guard = body.lock().await;
                let body = body_guard.take().ok_or(PyMiddlewareError::RequestGone)?;
                let body = hyper::body::to_bytes(body)
                    .await
                    .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
                let buf = body.clone();
                body_guard.replace(to_boxed(body));
                buf
            };
            // TODO(Perf): can we use `PyBytes` here?
            Ok(body.to_vec())
        })
    }

    /// Set the HTTP body of this response.
    #[setter]
    fn set_body(&mut self, buf: &[u8]) {
        self.body = Arc::new(Mutex::new(Some(to_boxed(buf.to_owned()))));
    }
}
