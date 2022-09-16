use std::collections::HashMap;

use http::Request;
use pyo3::prelude::*;

#[pyclass(name = "Request")]
#[derive(Debug, Clone)]
pub struct PyRequest {
    #[pyo3(get, set)]
    method: String,
    #[pyo3(get, set)]
    uri: String,
    #[pyo3(get, set)]
    pub headers: HashMap<String, String>,
}

impl PyRequest {
    pub fn new<B>(request: &Request<B>) -> Self {
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
        }
    }
}

#[pymethods]
impl PyRequest {
    #[new]
    fn newpy(method: String, uri: String, headers: Option<HashMap<String, String>>) -> Self {
        Self {
            method,
            uri,
            headers: headers.unwrap_or_default(),
        }
    }
}
