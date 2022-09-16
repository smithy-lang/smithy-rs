/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Python error definition.

use aws_smithy_types::date_time::{ConversionError, DateTimeParseError};
use pyo3::{exceptions::PyException, PyErr, create_exception};
use thiserror::Error;
use http::{Error as HttpError, status::InvalidStatusCode, header::ToStrError};

/// Python error that implements foreign errors.
#[derive(Error, Debug)]
pub enum Error {
    /// Implements `From<aws_smithy_types::date_time::DateTimeParseError>`.
    #[error("DateTimeConversion: {0}")]
    DateTimeConversion(#[from] ConversionError),
    /// Implements `From<aws_smithy_types::date_time::ConversionError>`.
    #[error("DateTimeParse: {0}")]
    DateTimeParse(#[from] DateTimeParseError),
    /// Http errors
    #[error("HTTP error: {0}")]
    Http(#[from] HttpError),
    /// Status code error
    #[error("{0}")]
    HttpStatusCode(#[from] InvalidStatusCode),
    #[error("{0}")]
    StrConversion(#[from] ToStrError )
}

create_exception!(smithy, PyError, PyException);

impl From<Error> for PyErr {
    fn from(other: Error) -> PyErr {
        PyError::new_err(other.to_string())
    }
}
