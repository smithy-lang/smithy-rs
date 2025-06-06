/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Python error definition.

#![allow(non_local_definitions)]

use aws_smithy_http_server::{
    body::{to_boxed, BoxBody},
    protocol::{
        aws_json_10::AwsJson1_0, aws_json_11::AwsJson1_1, rest_json_1::RestJson1, rest_xml::RestXml,
    },
    response::IntoResponse,
};
use aws_smithy_types::date_time::{ConversionError, DateTimeParseError};
use pyo3::{create_exception, exceptions::PyException as BasePyException, prelude::*};
use thiserror::Error;

/// Python error that implements foreign errors.
#[derive(Error, Debug)]
pub enum PyError {
    /// Implements `From<aws_smithy_types::date_time::DateTimeParseError>`.
    #[error("DateTimeConversion: {0}")]
    DateTimeConversion(#[from] ConversionError),
    /// Implements `From<aws_smithy_types::date_time::ConversionError>`.
    #[error("DateTimeParse: {0}")]
    DateTimeParse(#[from] DateTimeParseError),
}

create_exception!(smithy, PyException, BasePyException);

impl From<PyError> for PyErr {
    fn from(other: PyError) -> PyErr {
        PyException::new_err(other.to_string())
    }
}

/// Exception that can be thrown from a Python middleware.
///
/// It allows to specify a message and HTTP status code and implementing protocol specific capabilities
/// to build a [aws_smithy_http_server::response::Response] from it.
///
/// :param message str:
/// :param status_code typing.Optional\[int\]:
/// :rtype None:
#[pyclass(name = "MiddlewareException", extends = BasePyException)]
#[derive(Debug, Clone)]
pub struct PyMiddlewareException {
    /// :type str:
    #[pyo3(get, set)]
    message: String,

    /// :type int:
    #[pyo3(get, set)]
    status_code: u16,
}

#[pymethods]
impl PyMiddlewareException {
    /// Create a new [PyMiddlewareException].
    #[pyo3(text_signature = "($self, message, status_code=None)")]
    #[new]
    fn newpy(message: String, status_code: Option<u16>) -> Self {
        Self {
            message,
            status_code: status_code.unwrap_or(500),
        }
    }
}

impl From<PyErr> for PyMiddlewareException {
    fn from(other: PyErr) -> Self {
        // Try to extract `PyMiddlewareException` from `PyErr` and use that if succeed
        let middleware_err = Python::with_gil(|py| other.to_object(py).extract::<Self>(py));
        match middleware_err {
            Ok(err) => err,
            Err(_) => Self::newpy(other.to_string(), None),
        }
    }
}

impl IntoResponse<RestJson1> for PyMiddlewareException {
    fn into_response(self) -> http::Response<BoxBody> {
        http::Response::builder()
            .status(self.status_code)
            .header("Content-Type", "application/json")
            .header("X-Amzn-Errortype", "MiddlewareException")
            .body(to_boxed(self.json_body()))
            .expect("invalid HTTP response for `MiddlewareException`; please file a bug report under https://github.com/smithy-lang/smithy-rs/issues")
    }
}

impl IntoResponse<RestXml> for PyMiddlewareException {
    fn into_response(self) -> http::Response<BoxBody> {
        http::Response::builder()
            .status(self.status_code)
            .header("Content-Type", "application/xml")
            .body(to_boxed(self.xml_body()))
            .expect("invalid HTTP response for `MiddlewareException`; please file a bug report under https://github.com/smithy-lang/smithy-rs/issues")
    }
}

impl IntoResponse<AwsJson1_0> for PyMiddlewareException {
    fn into_response(self) -> http::Response<BoxBody> {
        http::Response::builder()
            .status(self.status_code)
            .header("Content-Type", "application/x-amz-json-1.0")
            // See https://smithy.io/2.0/aws/protocols/aws-json-1_0-protocol.html#empty-body-serialization
            .body(to_boxed(self.json_body()))
            .expect("invalid HTTP response for `MiddlewareException`; please file a bug report under https://github.com/smithy-lang/smithy-rs/issues")
    }
}

impl IntoResponse<AwsJson1_1> for PyMiddlewareException {
    fn into_response(self) -> http::Response<BoxBody> {
        http::Response::builder()
            .status(self.status_code)
            .header("Content-Type", "application/x-amz-json-1.1")
            // See https://smithy.io/2.0/aws/protocols/aws-json-1_1-protocol.html#empty-body-serialization
            .body(to_boxed(self.json_body()))
            .expect("invalid HTTP response for `MiddlewareException`; please file a bug report under https://github.com/smithy-lang/smithy-rs/issues")
    }
}

impl PyMiddlewareException {
    /// Serialize the body into a JSON object.
    fn json_body(&self) -> String {
        let mut out = String::new();
        let mut object = aws_smithy_json::serialize::JsonObjectWriter::new(&mut out);
        object.key("message").string(self.message.as_str());
        object.finish();
        out
    }

    /// Serialize the body into a XML object.
    fn xml_body(&self) -> String {
        let mut out = String::new();
        {
            let mut writer = aws_smithy_xml::encode::XmlWriter::new(&mut out);
            let root = writer
                .start_el("Error")
                .write_ns("http://s3.amazonaws.com/doc/2006-03-01/", None);
            let mut scope = root.finish();
            {
                let mut inner_writer = scope.start_el("Message").finish();
                inner_writer.data(self.message.as_ref());
            }
            scope.finish();
        }
        out
    }
}
