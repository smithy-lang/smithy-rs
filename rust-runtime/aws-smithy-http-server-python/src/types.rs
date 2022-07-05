/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Python wrapped types from aws-smithy-types.

use pyo3::prelude::*;

use crate::Error;

/// Python Wrapper for [aws_smithy_types::Blob].
#[pyclass]
#[derive(Debug, Clone, PartialEq)]
pub struct Blob(aws_smithy_types::Blob);

impl Blob {
    /// Creates a new blob from the given `input`.
    pub fn new<T: Into<Vec<u8>>>(input: T) -> Self {
        Self(aws_smithy_types::Blob::new(input))
    }

    /// Consumes the `Blob` and returns a `Vec<u8>` with its contents.
    pub fn into_inner(self) -> Vec<u8> {
        self.0.into_inner()
    }
}

impl AsRef<[u8]> for Blob {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

#[pymethods]
impl Blob {
    /// Create a new Python instance of `Blob`.
    #[new]
    pub fn pynew(input: Vec<u8>) -> Self {
        Self(aws_smithy_types::Blob::new(input))
    }

    /// Python getter for the `Blob` byte array.
    #[getter(data)]
    pub fn get_data(&self) -> &[u8] {
        self.as_ref()
    }

    /// Python setter for the `Blob` byte array.
    #[setter(data)]
    pub fn set_data(&mut self, data: Vec<u8>) {
        *self = Self::pynew(data);
    }
}

impl From<aws_smithy_types::Blob> for Blob {
    fn from(other: aws_smithy_types::Blob) -> Blob {
        Blob(other)
    }
}

impl From<Blob> for aws_smithy_types::Blob {
    fn from(other: Blob) -> aws_smithy_types::Blob {
        other.0
    }
}

/// Python Wrapper for [aws_smithy_types::date_time::DateTime].
#[pyclass]
#[derive(Debug, Clone, PartialEq)]
pub struct DateTime(aws_smithy_types::date_time::DateTime);

#[pyclass]
/// Formats for representing a `DateTime` in the Smithy protocols.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    /// RFC-3339 Date Time.
    DateTime,
    /// Date format used by the HTTP `Date` header, specified in RFC-7231.
    HttpDate,
    /// Number of seconds since the Unix epoch formatted as a floating point.
    EpochSeconds,
}

impl From<Format> for aws_smithy_types::date_time::Format {
    fn from(variant: Format) -> aws_smithy_types::date_time::Format {
        match variant {
            Format::DateTime => aws_smithy_types::date_time::Format::DateTime,
            Format::HttpDate => aws_smithy_types::date_time::Format::HttpDate,
            Format::EpochSeconds => aws_smithy_types::date_time::Format::EpochSeconds,
        }
    }
}

/// DateTime in time.
///
/// DateTime in time represented as seconds and sub-second nanos since
/// the Unix epoch (January 1, 1970 at midnight UTC/GMT).
#[pymethods]
impl DateTime {
    /// Creates a `DateTime` from a number of seconds since the Unix epoch.
    #[staticmethod]
    pub fn from_secs(epoch_seconds: i64) -> Self {
        Self(aws_smithy_types::date_time::DateTime::from_secs(
            epoch_seconds,
        ))
    }

    /// Creates a `DateTime` from a number of milliseconds since the Unix epoch.
    #[staticmethod]
    pub fn from_millis(epoch_millis: i64) -> Self {
        Self(aws_smithy_types::date_time::DateTime::from_secs(
            epoch_millis,
        ))
    }

    /// Creates a `DateTime` from a number of nanoseconds since the Unix epoch.
    #[staticmethod]
    pub fn from_nanos(epoch_nanos: i128) -> PyResult<Self> {
        Ok(Self(
            aws_smithy_types::date_time::DateTime::from_nanos(epoch_nanos)
                .map_err(Error::DateTimeConversion)?,
        ))
    }

    /// Read 1 date of `format` from `s`, expecting either `delim` or EOF.
    #[staticmethod]
    pub fn read(s: &str, format: Format, delim: char) -> PyResult<(Self, &str)> {
        let (self_, next) = aws_smithy_types::date_time::DateTime::read(s, format.into(), delim)
            .map_err(Error::DateTimeParse)?;
        Ok((Self(self_), next))
    }

    /// Creates a `DateTime` from a number of seconds and a fractional second since the Unix epoch.
    #[staticmethod]
    pub fn from_fractional_secs(epoch_seconds: i64, fraction: f64) -> Self {
        Self(aws_smithy_types::date_time::DateTime::from_fractional_secs(
            epoch_seconds,
            fraction,
        ))
    }

    /// Creates a `DateTime` from a number of seconds and sub-second nanos since the Unix epoch.
    #[staticmethod]
    pub fn from_secs_and_nanos(seconds: i64, subsecond_nanos: u32) -> Self {
        Self(aws_smithy_types::date_time::DateTime::from_secs_and_nanos(
            seconds,
            subsecond_nanos,
        ))
    }

    /// Creates a `DateTime` from an `f64` representing the number of seconds since the Unix epoch.
    #[staticmethod]
    pub fn from_secs_f64(epoch_seconds: f64) -> Self {
        Self(aws_smithy_types::date_time::DateTime::from_secs_f64(
            epoch_seconds,
        ))
    }

    /// Parses a `DateTime` from a string using the given `format`.
    #[staticmethod]
    pub fn from_str(s: &str, format: Format) -> PyResult<Self> {
        Ok(Self(
            aws_smithy_types::date_time::DateTime::from_str(s, format.into())
                .map_err(Error::DateTimeParse)?,
        ))
    }

    /// Returns the number of nanoseconds since the Unix epoch that this `DateTime` represents.
    pub fn as_nanos(&self) -> i128 {
        self.0.as_nanos()
    }

    /// Returns the `DateTime` value as an `f64` representing the seconds since the Unix epoch.
    pub fn as_secs_f64(&self) -> f64 {
        self.0.as_secs_f64()
    }

    /// Returns true if sub-second nanos is greater than zero.
    pub fn has_subsec_nanos(&self) -> bool {
        self.0.has_subsec_nanos()
    }

    /// Returns the epoch seconds component of the `DateTime`.
    pub fn secs(&self) -> i64 {
        self.0.secs()
    }

    /// Returns the sub-second nanos component of the `DateTime`.
    pub fn subsec_nanos(&self) -> u32 {
        self.0.subsec_nanos()
    }

    /// Converts the `DateTime` to the number of milliseconds since the Unix epoch.
    pub fn to_millis(&self) -> PyResult<i64> {
        Ok(self.0.to_millis().map_err(Error::DateTimeConversion)?)
    }
}

impl From<aws_smithy_types::DateTime> for DateTime {
    fn from(other: aws_smithy_types::DateTime) -> DateTime {
        DateTime(other)
    }
}

impl From<DateTime> for aws_smithy_types::DateTime {
    fn from(other: DateTime) -> aws_smithy_types::DateTime {
        other.0
    }
}

#[cfg(test)]
mod tests {
    use pyo3::py_run;

    use super::*;

    #[test]
    fn blob_can_be_used_in_python_when_initialized_in_rust() {
        crate::tests::initialize();
        Python::with_gil(|py| {
            let blob = Blob::new("some data".as_bytes().to_vec());
            let blob = PyCell::new(py, blob).unwrap();
            py_run!(
                py,
                blob,
                r#"
                assert blob.data == b"some data"
                assert len(blob.data) == 9
                blob.data = b"some other data"
                assert blob.data == b"some other data"
                assert len(blob.data) == 15
            "#
            );
        })
    }

    #[test]
    fn blob_can_be_initialized_in_python() {
        crate::tests::initialize();
        Python::with_gil(|py| {
            let types = PyModule::new(py, "types").unwrap();
            types.add_class::<Blob>().unwrap();
            py_run!(
                py,
                types,
                r#"
                blob = types.Blob(b"some data")
                assert blob.data == b"some data"
                assert len(blob.data) == 9
                blob.data = b"some other data"
                assert blob.data == b"some other data"
                assert len(blob.data) == 15
            "#
            );
        })
    }

    #[test]
    fn datetime_can_be_used_in_python_when_initialized_in_rust() {
        crate::tests::initialize();
        Python::with_gil(|py| {
            let datetime = DateTime::from_secs(100);
            let datetime = PyCell::new(py, datetime).unwrap();
            py_run!(py, datetime, "assert datetime.secs() == 100");
        })
    }

    #[test]
    fn datetime_can_by_initialized_in_python() {
        crate::tests::initialize();
        Python::with_gil(|py| {
            let types = PyModule::new(py, "types").unwrap();
            types.add_class::<DateTime>().unwrap();
            py_run!(
                py,
                types,
                "assert types.DateTime.from_secs(100).secs() == 100"
            );
        })
    }
}
