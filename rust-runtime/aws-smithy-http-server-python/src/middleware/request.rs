/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Python-compatible middleware [http::Request] implementation.

use std::mem;
use std::sync::Arc;

use aws_smithy_http_server::body::Body;
use http::{request::Parts, Request};
use pyo3::{
    exceptions::{PyRuntimeError, PyValueError},
    prelude::*,
};
use tokio::sync::Mutex;

use super::{PyHeaderMap, PyMiddlewareError};

/// Python-compatible [Request] object.
#[pyclass(name = "Request")]
#[derive(Debug)]
pub struct PyRequest {
    parts: Option<Parts>,
    headers: PyHeaderMap,
    body: Arc<Mutex<Option<Body>>>,
}

impl PyRequest {
    /// Create a new Python-compatible [Request] structure from the Rust side.
    pub fn new(request: Request<Body>) -> Self {
        let (mut parts, body) = request.into_parts();
        let headers = mem::take(&mut parts.headers);
        Self {
            parts: Some(parts),
            headers: PyHeaderMap::new(headers),
            body: Arc::new(Mutex::new(Some(body))),
        }
    }

    // Consumes self by taking the inner Request.
    // This method would have been `into_inner(self) -> Request<Body>`
    // but we can't do that because we are crossing Python boundary.
    pub fn take_inner(&mut self) -> Option<Request<Body>> {
        let headers = self.headers.take_inner()?;
        let mut parts = self.parts.take()?;
        parts.headers = headers;
        let body = {
            let body = mem::take(&mut self.body);
            let body = Arc::try_unwrap(body).ok()?;
            body.into_inner()?
        };
        Some(Request::from_parts(parts, body))
    }
}

#[pymethods]
impl PyRequest {
    /// Return the HTTP method of this request.
    ///
    /// :type str:
    #[getter]
    fn method(&self) -> PyResult<String> {
        self.parts
            .as_ref()
            .map(|parts| parts.method.to_string())
            .ok_or_else(|| PyMiddlewareError::RequestGone.into())
    }

    /// Return the URI of this request.
    ///
    /// :type str:
    #[getter]
    fn uri(&self) -> PyResult<String> {
        self.parts
            .as_ref()
            .map(|parts| parts.uri.to_string())
            .ok_or_else(|| PyMiddlewareError::RequestGone.into())
    }

    /// Sets the URI of this request.
    ///
    /// :type str:
    #[setter]
    fn set_uri(&mut self, uri_str: String) -> PyResult<()> {
        self.parts.as_mut().map_or_else(
            || Err(PyMiddlewareError::RequestGone.into()),
            |parts| {
                parts.uri = uri_str.parse().map_err(|e: http::uri::InvalidUri| {
                    PyValueError::new_err(format!(
                        "URI `{}` cannot be parsed. Error: {}",
                        uri_str, e
                    ))
                })?;
                Ok(())
            },
        )
    }

    /// Return the HTTP version of this request.
    ///
    /// :type str:
    #[getter]
    fn version(&self) -> PyResult<String> {
        self.parts
            .as_ref()
            .map(|parts| format!("{:?}", parts.version))
            .ok_or_else(|| PyMiddlewareError::RequestGone.into())
    }

    /// Return the HTTP headers of this request.
    ///
    /// :type typing.MutableMapping[str, str]:
    #[getter]
    fn headers(&self) -> PyHeaderMap {
        self.headers.clone()
    }

    /// Return the HTTP body of this request.
    /// Note that this is a costly operation because the whole request body is cloned.
    ///
    /// :type typing.Awaitable[bytes]:
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
