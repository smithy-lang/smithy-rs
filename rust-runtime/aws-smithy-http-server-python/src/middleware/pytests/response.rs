/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_http_server::body::to_boxed;
use aws_smithy_http_server_python::PyResponse;
use http::{Response, StatusCode, Version};
use pyo3::{
    prelude::*,
    py_run,
    types::{IntoPyDict, PyDict},
};

#[pyo3_asyncio::tokio::test]
async fn building_response_in_python() -> PyResult<()> {
    let response = Python::with_gil(|py| {
        let globals = [("Response", py.get_type::<PyResponse>())].into_py_dict(py);
        let locals = PyDict::new(py);

        py.run(
            r#"
response = Response(200, {"Content-Type": "application/json"}, b"hello world")
"#,
            Some(globals),
            Some(locals),
        )
        .unwrap();

        let py_response: Py<PyResponse> = locals
            .get_item("response")
            .expect("Python exception occurred during dictionary lookup")
            .unwrap()
            .extract()
            .unwrap();
        let response = py_response.borrow_mut(py).take_inner();
        response.unwrap()
    });

    assert_eq!(response.status(), StatusCode::OK);

    let headers = response.headers();
    {
        assert_eq!(headers.len(), 1);
        assert_eq!(headers.get("Content-Type").unwrap(), "application/json");
    }

    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    assert_eq!(body, "hello world");

    Ok(())
}

#[pyo3_asyncio::tokio::test]
async fn accessing_response_properties() -> PyResult<()> {
    let response = Response::builder()
        .status(StatusCode::IM_A_TEAPOT)
        .version(Version::HTTP_3)
        .header("X-Secret", "42")
        .body(to_boxed("hello world"))
        .expect("could not build response");
    let py_response = PyResponse::new(response);

    Python::with_gil(|py| {
        let res = PyCell::new(py, py_response)?;
        py_run!(
            py,
            res,
            r#"
assert res.status == 418
assert res.version == "HTTP/3.0"
assert res.headers["x-secret"] == "42"

assert res.headers.get("x-foo") == None
res.headers["x-foo"] = "bar"
assert res.headers["x-foo"] == "bar"
"#
        );
        Ok(())
    })
}

#[pyo3_asyncio::tokio::test]
async fn accessing_and_changing_response_body() -> PyResult<()> {
    let response = Response::builder()
        .body(to_boxed("hello world"))
        .expect("could not build response");
    let py_response = PyResponse::new(response);

    Python::with_gil(|py| {
        let module = PyModule::from_code(
            py,
            r#"
async def handler(res):
    assert bytes(await res.body) == b"hello world"

    res.body = b"hello world from middleware"
    assert bytes(await res.body) == b"hello world from middleware"
"#,
            "",
            "",
        )?;
        let handler = module.getattr("handler")?;

        let output = handler.call1((py_response,))?;
        Ok::<_, PyErr>(pyo3_asyncio::tokio::into_future(output))
    })??
    .await?;

    Ok(())
}
