/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::collections::HashMap;

use aws_smithy_http_server::body::Body;
use http::{Request, Version};
use pyo3::prelude::*;

#[pyclass(name = "HttpVersion")]
#[derive(PartialEq, PartialOrd, Copy, Clone, Eq, Ord, Hash)]
pub enum PyHttpVersion {
    Http09,
    Http10,
    Http11,
    H2,
    H3,
    __NonExhaustive,
}

#[pyclass(name = "Request")]
#[derive(Debug, Clone)]
pub struct PyRequest {
    #[pyo3(get, set)]
    method: String,
    #[pyo3(get, set)]
    uri: String,
    pub headers: HashMap<String, String>,
    version: Version,
}

impl PyRequest {
    pub fn new(request: &Request<Body>) -> Self {
        Self {
            method: request.method().to_string(),
            uri: request.uri().to_string(),
            headers: request
                .headers()
                .into_iter()
                .map(|(k, v)| -> (String, String) {
                    let name: String = k.as_str().to_string();
                    let value: String = String::from_utf8_lossy(v.as_bytes()).to_string();
                    (name, value)
                })
                .collect(),
            version: request.version(),
        }
    }
}

#[pymethods]
impl PyRequest {
    #[new]
    fn newpy(
        method: String,
        uri: String,
        headers: Option<HashMap<String, String>>,
        version: Option<PyHttpVersion>,
    ) -> Self {
        let version = version
            .map(|v| match v {
                PyHttpVersion::Http09 => Version::HTTP_09,
                PyHttpVersion::Http10 => Version::HTTP_10,
                PyHttpVersion::Http11 => Version::HTTP_11,
                PyHttpVersion::H2 => Version::HTTP_2,
                PyHttpVersion::H3 => Version::HTTP_3,
                _ => unreachable!(),
            })
            .unwrap_or(Version::HTTP_11);
        Self {
            method,
            uri,
            headers: headers.unwrap_or_default(),
            version,
        }
    }

    fn version(&self) -> PyHttpVersion {
        match self.version {
            Version::HTTP_09 => PyHttpVersion::Http09,
            Version::HTTP_10 => PyHttpVersion::Http10,
            Version::HTTP_11 => PyHttpVersion::Http11,
            Version::HTTP_2 => PyHttpVersion::H2,
            Version::HTTP_3 => PyHttpVersion::H3,
            _ => unreachable!(),
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
