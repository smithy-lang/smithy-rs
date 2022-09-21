/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Python-compatible middleware [http::Request] implementation.
use std::collections::HashMap;

use aws_smithy_http_server::body::Body;
use http::{Request, Version};
use pyo3::prelude::*;

/// Python compabible HTTP [Version].
#[pyclass(name = "HttpVersion")]
#[derive(PartialEq, PartialOrd, Copy, Clone, Eq, Ord, Hash)]
pub struct PyHttpVersion(Version);

#[pymethods]
impl PyHttpVersion {
    /// Extract the value of the HTTP [Version] into a string that
    /// can be used by Python.
    #[pyo3(text_signature = "($self)")]
    fn value(&self) -> &str {
        match self.0 {
            Version::HTTP_09 => "HTTP/0.9",
            Version::HTTP_10 => "HTTP/1.0",
            Version::HTTP_11 => "HTTP/1.1",
            Version::HTTP_2 => "HTTP/2.0",
            Version::HTTP_3 => "HTTP/3.0",
            _ => unreachable!(),
        }
    }
}

/// Python-compatible [Request] object.
///
/// For performance reasons, there is not support yet to pass the body to the Python middleware,
/// as it requires to consume and clone the body, which is a very expensive operation.
///
/// TODO(if customers request for it, we can implemented an opt-in functionality to also pass
/// the body around).
#[pyclass(name = "Request")]
#[pyo3(text_signature = "(request)")]
#[derive(Debug, Clone)]
pub struct PyRequest {
    #[pyo3(get, set)]
    method: String,
    #[pyo3(get, set)]
    uri: String,
    // TODO(investigate if using a PyDict can make the experience more idiomatic)
    // I'd like to be able to do request.headers.get("my-header") and
    // request.headers["my-header"] = 42 instead of implementing set_header() and get_header()
    // under pymethods. The same applies to response.
    pub(crate) headers: HashMap<String, String>,
    version: Version,
}

impl PyRequest {
    /// Create a new Python-compatible [Request] structure from the Rust side.
    ///
    /// This is done by cloning the headers, method, URI and HTTP version to let them be owned by Python.
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
    /// Create a new Python-compatible `Request` object from the Python side.
    fn newpy(
        method: String,
        uri: String,
        headers: Option<HashMap<String, String>>,
        version: Option<PyHttpVersion>,
    ) -> Self {
        let version = version.map(|v| v.0).unwrap_or(Version::HTTP_11);
        Self {
            method,
            uri,
            headers: headers.unwrap_or_default(),
            version,
        }
    }

    /// Return the HTTP version of this request.
    #[pyo3(text_signature = "($self)")]
    fn version(&self) -> String {
        PyHttpVersion(self.version).value().to_string()
    }

    /// Return the HTTP headers of this request.
    /// TODO(can we use `Py::clone_ref()` to prevent cloning the hashmap?)
    #[pyo3(text_signature = "($self)")]
    fn headers(&self) -> HashMap<String, String> {
        self.headers.clone()
    }

    /// Insert a new key/value into this request's headers.
    #[pyo3(text_signature = "($self, key, value)")]
    fn set_header(&mut self, key: &str, value: &str) {
        self.headers.insert(key.to_string(), value.to_string());
    }

    /// Return a header value of this request.
    #[pyo3(text_signature = "($self, key)")]
    fn get_header(&self, key: &str) -> Option<&String> {
        self.headers.get(key)
    }
}
