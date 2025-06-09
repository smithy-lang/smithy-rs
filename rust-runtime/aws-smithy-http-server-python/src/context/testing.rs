/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Testing utilities for [PyContext].

use std::ffi::CString;

use http::{header::HeaderName, HeaderMap, HeaderValue};
use pyo3::{
    types::{PyAnyMethods, PyDict, PyModule, PyModuleMethods},
    PyErr, Python,
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
        let c_string = CString::new(code).expect("`code` cannot be converted to CString");
        py.run(c_string.as_c_str(), Some(&globals), Some(&locals))?;
        let context = locals
            .get_item("ctx")
            .expect("Python exception occurred during dictionary lookup")
            .unbind();
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
