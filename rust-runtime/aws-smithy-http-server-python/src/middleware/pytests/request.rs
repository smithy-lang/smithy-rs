/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_http_server_python::PyRequest;
use http::{Request, Version};
use hyper::Body;
use pyo3::{exceptions::PyValueError, prelude::*, py_run};

#[pyo3_async_runtimes::tokio::test]
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

assert req.headers.get("x-foo") == None
req.headers["x-foo"] = "bar"
assert req.headers["x-foo"] == "bar"
"#
        );
        Ok(())
    })
}

#[pyo3_async_runtimes::tokio::test]
async fn accessing_and_changing_request_body() -> PyResult<()> {
    let request = Request::builder()
        .body(Body::from("hello world"))
        .expect("could not build request");
    let py_request = PyRequest::new(request);

    Python::with_gil(|py| {
        let module = PyModule::from_code(
            py,
            r#"
async def handler(req):
    # TODO(Ergonomics): why we need to wrap with `bytes`?
    assert bytes(await req.body) == b"hello world"

    req.body = b"hello world from middleware"
    assert bytes(await req.body) == b"hello world from middleware"
"#,
            "",
            "",
        )?;
        let handler = module.getattr("handler")?;

        let output = handler.call1((py_request,))?;
        Ok::<_, PyErr>(pyo3_async_runtimes::tokio::into_future(output))
    })??
    .await?;

    Ok(())
}

#[pyo3_async_runtimes::tokio::test]
async fn accessing_and_changing_request_uri() -> PyResult<()> {
    let request = Request::builder()
        .uri("/op1")
        .body(Body::from("hello world"))
        .expect("could not build request");
    let py_request = PyRequest::new(request);

    // Call an async Python method to change the URI and return it.
    let modified_req = Python::with_gil(|py| {
        let module = PyModule::from_code(
            py,
            r#"
async def handler(req):
    assert req.uri == "/op1"
    # add a trailing slash to the uri
    req.uri = "/op1/"
    assert req.uri == "/op1/"
    return req
"#,
            "",
            "",
        )?;

        let req_ref = PyCell::new(py, py_request)?;
        let handler = module.getattr("handler")?;
        let output = handler.call1((req_ref,))?;

        Ok::<_, PyErr>(pyo3_async_runtimes::tokio::into_future(output))
    })??
    .await?;

    // Confirm that the URI has been changed when the modified PyRequest instance
    // from Python is converted into a http::Request<> instance.
    Python::with_gil(|py| {
        let request_cell: &PyCell<PyRequest> = modified_req.downcast(py)?;
        let mut request = request_cell.borrow_mut();
        let http_request = request
            .take_inner()
            .ok_or_else(|| PyValueError::new_err("inner http request has already been consumed"))?;
        assert_eq!(http_request.uri(), "/op1/");

        Ok::<_, PyErr>(())
    })?;

    Ok(())
}
