/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Python error definition.

use aws_smithy_http_server::protocols::Protocol;
use aws_smithy_http_server::{body::to_boxed, response::Response};
use aws_smithy_types::date_time::{ConversionError, DateTimeParseError};
use pyo3::{create_exception, exceptions::PyException as BasePyException, prelude::*, PyErr};
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
#[pyclass(name = "MiddlewareException", extends = BasePyException)]
#[pyo3(text_signature = "(message, status_code)")]
#[derive(Debug, Clone)]
pub struct PyMiddlewareException {
    #[pyo3(get, set)]
    message: String,
    #[pyo3(get, set)]
    status_code: u16,
}

#[pymethods]
impl PyMiddlewareException {
    /// Create a new [PyMiddlewareException].
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
        Self::newpy(other.to_string(), None)
    }
}

impl PyMiddlewareException {
    /// Convert the exception into a [Response], following the [Protocol] specification.
    pub(crate) fn into_response(self, protocol: Protocol) -> Response {
        let body = to_boxed(match protocol {
            Protocol::RestJson1 => self.json_body(),
            Protocol::RestXml => self.xml_body(),
            // See https://awslabs.github.io/smithy/1.0/spec/aws/aws-json-1_0-protocol.html#empty-body-serialization
            Protocol::AwsJson10 => self.json_body(),
            // See https://awslabs.github.io/smithy/1.0/spec/aws/aws-json-1_1-protocol.html#empty-body-serialization
            Protocol::AwsJson11 => self.json_body(),
        });

        let mut builder = http::Response::builder();
        builder = builder.status(self.status_code);

        match protocol {
            Protocol::RestJson1 => {
                builder = builder
                    .header("Content-Type", "application/json")
                    .header("X-Amzn-Errortype", "MiddlewareException");
            }
            Protocol::RestXml => builder = builder.header("Content-Type", "application/xml"),
            Protocol::AwsJson10 => {
                builder = builder.header("Content-Type", "application/x-amz-json-1.0")
            }
            Protocol::AwsJson11 => {
                builder = builder.header("Content-Type", "application/x-amz-json-1.1")
            }
        }

        builder.body(body).expect("invalid HTTP response for `MiddlewareException`; please file a bug report under https://github.com/awslabs/smithy-rs/issues")
    }

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
