use aws_smithy_http_server_python::PyRequest;
use http::{Request, Version};
use hyper::Body;
use pyo3::{prelude::*, py_run};

#[pyo3_asyncio::tokio::test]
async fn accessing_request_properties() -> PyResult<()> {
    let request = Request::builder()
        .method("POST")
        .uri("https://www.rust-lang.org/")
        .header("Accept-Encoding", "*")
        .header("X-Custom", "42")
        .version(Version::HTTP_2)
        .body(Body::from("hello world"))
        .expect("could not build request");
    let py_request = PyRequest::new(request);

    Python::with_gil(|py| {
        let req = PyCell::new(py, py_request)?;
        py_run!(
            py,
            req,
            r#"
            assert req.method == "POST"
            assert req.uri == "https://www.rust-lang.org/"
            assert req.headers["accept-encoding"] == "*"
            assert req.headers["x-custom"] == "42"
            assert req.version == "HTTP/2.0"
        "#
        );
        Ok(())
    })
}

// #[pyo3_asyncio::tokio::test]
// async fn accessing_request_body() -> PyResult<()> {
//     todo!()
// }
