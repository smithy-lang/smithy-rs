/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{collections::HashMap, convert::TryInto};

use aws_smithy_http_server::body::{to_boxed, BoxBody};
use http::{Response, StatusCode};
use pyo3::prelude::*;

#[pyclass(name = "Response")]
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
    #[new]
    fn newpy(status: u16, headers: Option<HashMap<String, String>>, body: Option<Vec<u8>>) -> Self {
        Self {
            status,
            body: body.unwrap_or_default(),
            headers: headers.unwrap_or_default(),
        }
    }

    fn headers(&self) -> HashMap<String, String> {
        self.headers.clone()
    }

    fn set_header(&mut self, key: &str, value: &str) {
        self.headers.insert(key.to_string(), value.to_string());
    }

    fn get_header(&self, key: &str) -> Option<&String> {
        self.headers.get(key)
    }
}

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
