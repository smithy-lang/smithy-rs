/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Python-compatible middleware [http::Response] implementation.

use std::collections::HashMap;
use std::mem;
use std::sync::Arc;

use aws_smithy_http_server::body::{to_boxed, BoxBody};
use http::{response::Parts, Response};
use pyo3::{exceptions::PyRuntimeError, prelude::*};
use tokio::sync::Mutex;

use super::{PyHeaderMap, PyMiddlewareError};

/// Python-compatible [Response] object.
///
/// :param status int:
/// :param headers typing.Optional[typing.Dict[str, str]]:
/// :param body typing.Optional[bytes]:
/// :rtype None:
#[pyclass(name = "Response")]
pub struct PyResponse {
    parts: Option<Parts>,
    headers: PyHeaderMap,
    body: Arc<Mutex<Option<BoxBody>>>,
}

impl PyResponse {
    /// Create a new Python-compatible [Response] structure from the Rust side.
    pub fn new(response: Response<BoxBody>) -> Self {
        let (mut parts, body) = response.into_parts();
        let headers = mem::take(&mut parts.headers);
        Self {
            parts: Some(parts),
            headers: PyHeaderMap::new(headers),
            body: Arc::new(Mutex::new(Some(body))),
        }
    }

    // Consumes self by taking the inner Response.
    // This method would have been `into_inner(self) -> Response<BoxBody>`
    // but we can't do that because we are crossing Python boundary.
    pub fn take_inner(&mut self) -> Option<Response<BoxBody>> {
        let headers = self.headers.take_inner()?;
        let mut parts = self.parts.take()?;
        parts.headers = headers;
        let body = {
            let body = mem::take(&mut self.body);
            let body = Arc::try_unwrap(body).ok()?;
            body.into_inner().take()?
        };
        Some(Response::from_parts(parts, body))
    }
}

#[pymethods]
impl PyResponse {
    /// Python-compatible [Response] object from the Python side.
    #[pyo3(text_signature = "($self, status, headers=None, body=None)")]
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
    ///
    /// :type int:
    #[getter]
    fn status(&self) -> PyResult<u16> {
        self.parts
            .as_ref()
            .map(|parts| parts.status.as_u16())
            .ok_or_else(|| PyMiddlewareError::ResponseGone.into())
    }

    /// Return the HTTP version of this response.
    ///
    /// :type str:
    #[getter]
    fn version(&self) -> PyResult<String> {
        self.parts
            .as_ref()
            .map(|parts| format!("{:?}", parts.version))
            .ok_or_else(|| PyMiddlewareError::ResponseGone.into())
    }

    /// Return the HTTP headers of this response.
    ///
    /// :type typing.MutableMapping[str, str]:
    #[getter]
    fn headers(&self) -> PyHeaderMap {
        self.headers.clone()
    }

    /// Return the HTTP body of this response.
    /// Note that this is a costly operation because the whole response body is cloned.
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
