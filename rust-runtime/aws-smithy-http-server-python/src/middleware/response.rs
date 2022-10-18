/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Python-compatible middleware [http::Response] implementation.

use std::collections::HashMap;

use aws_smithy_http_server::body::{to_boxed, BoxBody};
use http::Response;
use pyo3::{exceptions::PyRuntimeError, prelude::*};

/// Python-compatible [Response] object.
#[pyclass(name = "Response")]
#[pyo3(text_signature = "(status, headers, body)")]
pub struct PyResponse(Option<Response<BoxBody>>);

impl PyResponse {
    /// Create a new Python-compatible [Response] structure from the Rust side.
    pub fn new(response: Response<BoxBody>) -> Self {
        Self(Some(response))
    }

    pub fn take_inner(&mut self) -> Option<Response<BoxBody>> {
        self.0.take()
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

        Ok(Self(Some(response)))
    }
}
