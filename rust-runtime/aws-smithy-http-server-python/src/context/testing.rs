/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Testing utilities for [PyContext].

use http::{header::HeaderName, HeaderMap, HeaderValue};
use pyo3::{
    types::{PyDict, PyModule},
    IntoPy, PyErr, Python,
};

use super::PyContext;

pub fn get_context(code: &str) -> PyContext {
    let inner = Python::with_gil(|py| {
        let globals = PyModule::import(py, "__main__")?.dict();
        globals.set_item("typing", py.import("typing")?)?;
        globals.set_item(
            "LambdaContext",
            py.get_type::<crate::lambda::PyLambdaContext>(),
        )?;
        let locals = PyDict::new(py);
        py.run(code, Some(globals), Some(locals))?;
        let context = locals
            .get_item("ctx")
            .expect("Python exception occurred during dictionary lookup")
            .expect("you should assing your context class to `ctx` variable")
            .into_py(py);
        Ok::<_, PyErr>(context)
    })
    .unwrap();
    PyContext::new(inner).unwrap()
}

pub fn lambda_ctx(req_id: &'static str, deadline_ms: &'static str) -> lambda_http::Context {
    let headers = HeaderMap::from_iter([
        (
            HeaderName::from_static("lambda-runtime-aws-request-id"),
            HeaderValue::from_static(req_id),
        ),
        (
            HeaderName::from_static("lambda-runtime-deadline-ms"),
            HeaderValue::from_static(deadline_ms),
        ),
    ]);
    lambda_http::Context::try_from(headers).unwrap()
}
