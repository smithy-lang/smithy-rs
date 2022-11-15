/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Testing utilities for [PyContext].

use http::{HeaderMap, HeaderValue};
use lambda_http::Context;
use pyo3::{
    types::{PyDict, PyModule},
    IntoPy, PyErr, Python,
};

use super::{PyContext, PyLambdaContext};

pub fn get_context(code: &str) -> PyContext {
    let inner = Python::with_gil(|py| {
        let globals = PyModule::import(py, "__main__")?.dict();
        globals.set_item("typing", py.import("typing")?)?;
        globals.set_item("LambdaContext", py.get_type::<PyLambdaContext>())?;
        let locals = PyDict::new(py);
        py.run(code, Some(globals), Some(locals))?;
        let context = locals
            .get_item("ctx")
            .expect("you should assing your context class to `ctx` variable")
            .into_py(py);
        Ok::<_, PyErr>(context)
    })
    .unwrap();
    PyContext::new(inner).unwrap()
}

pub fn lambda_ctx(req_id: &'static str, deadline_ms: &'static str) -> Context {
    let mut headers = HeaderMap::new();
    headers.insert(
        "lambda-runtime-aws-request-id",
        HeaderValue::from_static(req_id),
    );
    headers.insert(
        "lambda-runtime-deadline-ms",
        HeaderValue::from_static(deadline_ms),
    );
    Context::try_from(headers).unwrap()
}

pub fn py_lambda_ctx(req_id: &'static str, deadline_ms: &'static str) -> PyLambdaContext {
    PyLambdaContext::new(lambda_ctx(req_id, deadline_ms))
}
