/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Python-compatible middleware [http::Request] implementation.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use aws_smithy_http_server::body::Body;
use http::{header::HeaderName, request::Parts, HeaderValue, Request};
use pyo3::{exceptions::PyRuntimeError, prelude::*};
use tokio::sync::Mutex;

use super::PyMiddlewareError;

/// Python-compatible [Request] object.
#[pyclass(name = "Request")]
#[pyo3(text_signature = "(request)")]
#[derive(Debug)]
pub struct PyRequest {
    parts: Option<Parts>,
    body: Arc<Mutex<Option<Body>>>,
}

impl PyRequest {
    /// Create a new Python-compatible [Request] structure from the Rust side.
    pub fn new(request: Request<Body>) -> Self {
        let (parts, body) = request.into_parts();
        Self {
            parts: Some(parts),
            body: Arc::new(Mutex::new(Some(body))),
        }
    }

    pub fn take_inner(&mut self) -> Option<Request<Body>> {
        let parts = self.parts.take()?;
        let body = std::mem::replace(&mut self.body, Arc::new(Mutex::new(None)));
        let body = Arc::try_unwrap(body).ok()?;
        let body = body.into_inner().take()?;
        Some(Request::from_parts(parts, body))
    }
}

#[pymethods]
impl PyRequest {
    /// Return the HTTP method of this request.
    #[getter]
    fn method(&self) -> PyResult<String> {
        self.parts
            .as_ref()
            .map(|parts| parts.method.to_string())
            .ok_or_else(|| PyMiddlewareError::RequestGone.into())
    }

    /// Return the URI of this request.
    #[getter]
    fn uri(&self) -> PyResult<String> {
        self.parts
            .as_ref()
            .map(|parts| parts.uri.to_string())
            .ok_or_else(|| PyMiddlewareError::RequestGone.into())
    }

    /// Return the HTTP version of this request.
    #[getter]
    fn version(&self) -> PyResult<String> {
        self.parts
            .as_ref()
            .map(|parts| format!("{:?}", parts.version))
            .ok_or_else(|| PyMiddlewareError::RequestGone.into())
    }

    /// Return the HTTP headers of this request.
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
            .ok_or_else(|| PyMiddlewareError::RequestGone.into())
    }

    /// Insert a new key/value into this request's headers.
    /// TODO(investigate if using a PyDict can make the experience more idiomatic)
    /// I'd like to be able to do request.headers.get("my-header") and
    /// request.headers["my-header"] = 42 instead of implementing set_header() and get_header()
    /// under pymethods. The same applies to response.
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
            None => Err(PyMiddlewareError::RequestGone.into()),
        }
    }

    /// Return the HTTP body of this request.
    /// Note that this is a costly operation because the whole request body is cloned.
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
                body_guard.replace(Body::from(body));
                buf
            };
            // TODO(Perf): can we use `PyBytes` here?
            Ok(body.to_vec())
        })
    }

    /// Set the HTTP body of this request.
    #[setter]
    fn set_body(&mut self, buf: &[u8]) {
        self.body = Arc::new(Mutex::new(Some(Body::from(buf.to_owned()))));
    }
}
