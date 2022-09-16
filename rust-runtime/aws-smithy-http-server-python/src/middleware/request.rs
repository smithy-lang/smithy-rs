use bytes::Bytes;
use http::Request;
use pyo3::prelude::*;

#[pyclass(name = "Request")]
#[derive(Debug)]
pub struct PyRequest(Request<Bytes>);

impl PyRequest {
    pub fn new<B>(request: &Request<B>) -> Self {
        let mut self_ = Request::builder()
            .uri(request.uri())
            .method(request.method())
            .body(Bytes::new())
            .unwrap();
        let headers = self_.headers_mut();
        *headers = request.headers().clone();
        Self(self_)
    }

    pub fn new_with_body<B>(request: &Request<B>) -> Self {
        let mut self_ = Request::builder()
            .uri(request.uri())
            .method(request.method())
            .body(Bytes::new())
            .unwrap();
        let headers = self_.headers_mut();
        *headers = request.headers().clone();
        Self(self_)
    }
}

impl Clone for PyRequest {
    fn clone(&self) -> Self {
        let mut request = Request::builder()
            .uri(self.0.uri())
            .method(self.0.method())
            .body(self.0.body().clone())
            .unwrap();
        let headers = request.headers_mut();
        *headers = self.0.headers().clone();
        Self(request)
    }
}

#[pymethods]
impl PyRequest {
    fn method(&self) -> String {
        self.0.method().to_string()
    }

    fn uri(&self) -> String {
        self.0.uri().to_string()
    }

    fn get_header(&self, name: &str) -> Option<String> {
        let value = self.0.headers().get(name);
        match value {
            Some(v) => match v.to_str() {
                Ok(v) => Some(v.to_string()),
                Err(_) => None,
            },
            None => None,
        }
    }
}
