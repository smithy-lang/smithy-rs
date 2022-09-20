/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Python-compatible middleware [http::Request] implementation.
use std::{collections::HashMap, convert::TryInto};

use aws_smithy_http_server::body::{to_boxed, BoxBody};
use http::{Response, StatusCode};
use pyo3::prelude::*;

/// Python-compatible [Response] object.
///
/// For performance reasons, there is not support yet to pass the body to the Python middleware,
/// as it requires to consume and clone the body, which is a very expensive operation.
///
// TODO(if customers request for it, we can implemented an opt-in functionality to also pass
// the body around).
#[pyclass(name = "Response")]
#[pyo3(text_signature = "(status, headers, body)")]
#[derive(Debug, Clone)]
pub struct PyResponse {
    #[pyo3(get, set)]
    status: u16,
    #[pyo3(get, set)]
    body: Vec<u8>,
    headers: HashMap<String, String>,
}

#[pymethods]
impl PyResponse {
    /// Python-compatible [Response] object from the Python side.
    #[new]
    fn newpy(status: u16, headers: Option<HashMap<String, String>>, body: Option<Vec<u8>>) -> Self {
        Self {
            status,
            body: body.unwrap_or_default(),
            headers: headers.unwrap_or_default(),
        }
    }

    /// Return the HTTP headers of this response.
    // TODO(can we use `Py::clone_ref()` to prevent cloning the hashmap?)
    #[pyo3(text_signature = "($self)")]
    fn headers(&self) -> HashMap<String, String> {
        self.headers.clone()
    }

    /// Insert a new key/value into this response's headers.
    #[pyo3(text_signature = "($self, key, value)")]
    fn set_header(&mut self, key: &str, value: &str) {
        self.headers.insert(key.to_string(), value.to_string());
    }

    /// Return a header value of this response.
    #[pyo3(text_signature = "($self, key)")]
    fn get_header(&self, key: &str) -> Option<&String> {
        self.headers.get(key)
    }
}

/// Allow to convert between a [PyResponse] and a [Response].
impl From<PyResponse> for Response<BoxBody> {
    fn from(pyresponse: PyResponse) -> Self {
        let mut response = Response::builder()
            .status(
                StatusCode::from_u16(pyresponse.status)
                    .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            )
            .body(to_boxed(pyresponse.body))
            .unwrap_or_default();
        match (&pyresponse.headers).try_into() {
            Ok(headers) => *response.headers_mut() = headers,
            Err(e) => tracing::error!("Error extracting HTTP headers from PyResponse: {e}"),
        };
        response
    }
}
