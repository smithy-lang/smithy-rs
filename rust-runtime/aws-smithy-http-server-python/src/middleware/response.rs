use std::{
    collections::HashMap,
    convert::{TryFrom, TryInto}, str::FromStr,
};

use aws_smithy_http_server::body::{to_boxed, BoxBody};
use http::{HeaderMap, HeaderValue, Response, StatusCode, header::HeaderName};
use pyo3::prelude::*;

use crate::error::PyError;

#[pyclass(name = "Response")]
#[derive(Debug, Clone)]
pub struct PyResponse {
    #[pyo3(get, set)]
    status: u16,
    #[pyo3(get, set)]
    body: Vec<u8>,
    #[pyo3(get, set)]
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
}

impl From<PyResponse> for Response<BoxBody> {
    fn from(val: PyResponse) -> Self {
        let mut response = Response::builder()
            .status(StatusCode::from_u16(val.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))
            .body(to_boxed(val.body))
            .unwrap_or_default();
        if let Ok(headers) = (&val.headers).try_into() {
            *response.headers_mut() = headers;
        }
        response
    }
}
