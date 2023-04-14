/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::time::Duration;

use http::{HeaderName, Method};

#[pyo3::pyclass(name = "TowerLayersConfig")]
#[derive(Debug, Clone, Default)]
pub struct PyTowerLayersConfig {
    pub timeout: Option<PyTimeoutLayerConfig>,
    pub instrument: Option<PyInstrumentLayerConfig>,
    pub request_id: Option<PyRequestIdLayerConfig>,
    pub cors: Option<PyCorsLayerConfig>,
}

#[pyo3::pyclass(name = "InstrumentLayerConfig")]
#[derive(Debug, Clone, Default)]
pub struct PyInstrumentLayerConfig;

#[pyo3::pyclass(name = "RequestIdLayerConfig")]
#[derive(Debug, Clone, Default)]
pub struct PyRequestIdLayerConfig {
    pub header_key: Option<HeaderName>,
}

#[pyo3::pymethods]
impl PyRequestIdLayerConfig {
    #[new]
    fn newpy(header_key: Option<&str>) -> pyo3::PyResult<Self> {
        let header_key = match header_key {
            Some(h) => Some(
                HeaderName::from_bytes(h.as_bytes())
                    .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?,
            ),
            None => None,
        };
        Ok(Self { header_key })
    }
}

/// [tower_http::timeout::TimeoutLayer] configuration wrapper.
#[pyo3::pyclass(name = "TimeoutLayerConfig")]
#[derive(Debug, Clone)]
pub struct PyTimeoutLayerConfig {
    pub timeout: Duration,
}

impl Default for PyTimeoutLayerConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(300),
        }
    }
}

#[pyo3::pymethods]
impl PyTimeoutLayerConfig {
    #[new]
    fn newpy(timeout_ms: u64) -> Self {
        Self {
            timeout: Duration::from_millis(timeout_ms),
        }
    }
}

#[pyo3::pyclass(name = "CorsLayerConfig")]
#[derive(Debug, Clone, Default)]
pub struct PyCorsLayerConfig {
    pub allow_credentials: bool,
    pub allow_headers: Option<Vec<HeaderName>>,
    pub allow_methods: Option<Vec<Method>>,
    pub allow_origins: Option<Vec<String>>,
}

#[pyo3::pymethods]
impl PyCorsLayerConfig {
    #[new]
    fn newpy(
        allow_credentials: Option<bool>,
        allow_headers: Option<Vec<String>>,
        allow_methods: Option<Vec<String>>,
        allow_origins: Option<Vec<String>>,
    ) -> pyo3::PyResult<Self> {
        let allow_credentials = allow_credentials.unwrap_or(false);
        let allow_headers = match allow_headers {
            Some(headers) => {
                let mut allow_headers = vec![];
                for header in headers {
                    allow_headers.push(
                        header
                            .parse::<HeaderName>()
                            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?,
                    );
                }
                if allow_headers.is_empty() {
                    None
                } else {
                    Some(allow_headers)
                }
            }
            None => None,
        };
        let allow_methods = match allow_methods {
            Some(methods) => {
                let mut allow_methods = vec![];
                for method in methods {
                    allow_methods.push(
                        method
                            .parse::<Method>()
                            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?,
                    );
                }
                if allow_methods.is_empty() {
                    None
                } else {
                    Some(allow_methods)
                }
            }
            None => None,
        };
        Ok(Self {
            allow_credentials,
            allow_headers,
            allow_methods,
            allow_origins,
        })
    }
}
