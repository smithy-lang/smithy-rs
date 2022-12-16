/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

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

assert req.headers.get("x-foo") == None
req.headers["x-foo"] = "bar"
assert req.headers["x-foo"] == "bar"
"#
        );
        Ok(())
    })
}

#[pyo3_asyncio::tokio::test]
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
        Ok::<_, PyErr>(pyo3_asyncio::tokio::into_future(output))
    })??
    .await?;

    Ok(())
}
