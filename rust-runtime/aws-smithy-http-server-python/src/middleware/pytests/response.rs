/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_http_server_python::PyResponse;
use http::StatusCode;
use pyo3::{
    prelude::*,
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

        let py_response: Py<PyResponse> = locals.get_item("response").unwrap().extract().unwrap();
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
