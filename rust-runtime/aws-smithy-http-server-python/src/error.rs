/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Python error definition.

use aws_smithy_types::date_time::{ConversionError, DateTimeParseError};
use pyo3::{exceptions::PyException, PyErr};
use thiserror::Error;

/// Python error that implements foreign errors.
#[derive(Error, Debug)]
pub enum Error {
    /// Implements `From<aws_smithy_types::date_time::DateTimeParseError>`.
    #[error("DateTimeConversion: {0}")]
    DateTimeConversion(#[from] ConversionError),
    /// Implements `From<aws_smithy_types::date_time::ConversionError>`.
    #[error("DateTimeParse: {0}")]
    DateTimeParse(#[from] DateTimeParseError),
}

impl From<Error> for PyErr {
    fn from(other: Error) -> PyErr {
        PyException::new_err(other.to_string())
    }
}
