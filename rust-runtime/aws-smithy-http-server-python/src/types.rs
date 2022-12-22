/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Python wrapped types from aws-smithy-types and aws-smithy-http.

use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use bytes::Bytes;
use pyo3::{
    exceptions::{PyRuntimeError, PyStopIteration},
    iter::IterNextOutput,
    prelude::*,
    pyclass::IterANextOutput,
};
use tokio::sync::Mutex;
use tokio_stream::StreamExt;

use crate::PyError;

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

impl<'blob> From<&'blob Blob> for &'blob aws_smithy_types::Blob {
    fn from(other: &'blob Blob) -> &'blob aws_smithy_types::Blob {
        &other.0
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

impl DateTime {
    /// Formats the `DateTime` to a string using the given `format`.
    ///
    /// Returns an error if the given `DateTime` cannot be represented by the desired format.
    pub fn fmt(
        &self,
        format: aws_smithy_types::date_time::Format,
    ) -> Result<String, aws_smithy_types::date_time::DateTimeFormatError> {
        self.0.fmt(format)
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
                .map_err(PyError::DateTimeConversion)?,
        ))
    }

    /// Read 1 date of `format` from `s`, expecting either `delim` or EOF.
    #[staticmethod]
    pub fn read(s: &str, format: Format, delim: char) -> PyResult<(Self, &str)> {
        let (self_, next) = aws_smithy_types::date_time::DateTime::read(s, format.into(), delim)
            .map_err(PyError::DateTimeParse)?;
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
                .map_err(PyError::DateTimeParse)?,
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
        Ok(self.0.to_millis().map_err(PyError::DateTimeConversion)?)
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

impl<'date> From<&'date DateTime> for &'date aws_smithy_types::DateTime {
    fn from(other: &'date DateTime) -> &'date aws_smithy_types::DateTime {
        &other.0
    }
}

/// Python Wrapper for [aws_smithy_http::byte_stream::ByteStream].
///
/// ByteStream provides misuse-resistant primitives to make it easier to handle common patterns with streaming data.
///
/// On the Rust side, The Python implementation wraps the original [ByteStream](aws_smithy_http::byte_stream::ByteStream)
/// in a clonable structure and implements the [Stream](futures::stream::Stream) trait for it to
/// allow Rust to handle the type transparently.
///
/// On the Python side both sync and async iterators are exposed by implementing `__iter__()` and `__aiter__()` magic methods,
/// which allows to just loop over the stream chunks.
///
/// ### Example of async streaming:
///
/// ```python
///     stream = await ByteStream.from_path("/tmp/music.mp3")
///     async for chunk in stream:
///         print(chunk)
/// ```
///
/// ### Example of sync streaming:
///
/// ```python
///     stream = ByteStream.from_stream_blocking("/tmp/music.mp3")
///     for chunk in stream:
///         print(chunk)
/// ```
///
/// The main difference between the two implementations is that the async one is scheduling the Python coroutines as Rust futures,
/// effectively maintaining the asyncronous behavior that Rust exposes, while the sync one is blocking the Tokio runtime to be able
/// to await one chunk at a time.
///
/// The original Rust [ByteStream](aws_smithy_http::byte_stream::ByteStream) is wrapped inside a `Arc<Mutex>` to allow the type to be
/// [Clone] (required by PyO3) and to allow internal mutability, required to fetch the next chunk of data.
#[pyclass]
#[derive(Debug, Clone)]
pub struct ByteStream(Arc<Mutex<aws_smithy_http::byte_stream::ByteStream>>);

impl futures::stream::Stream for ByteStream {
    type Item = Result<Bytes, aws_smithy_http::byte_stream::error::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let stream = self.0.lock();
        tokio::pin!(stream);
        match stream.poll(cx) {
            Poll::Ready(mut stream) => Pin::new(&mut *stream).poll_next(cx),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Return a new data chunk from the stream.
async fn yield_data_chunk(
    body: Arc<Mutex<aws_smithy_http::byte_stream::ByteStream>>,
) -> PyResult<Bytes> {
    let mut stream = body.lock().await;
    match stream.next().await {
        Some(bytes) => bytes.map_err(|e| PyRuntimeError::new_err(e.to_string())),
        None => Err(PyStopIteration::new_err("stream exhausted")),
    }
}

impl ByteStream {
    /// Construct a new [ByteStream](aws_smithy_http::byte_stream::ByteStream) from a
    /// [SdkBody](aws_smithy_http::body::SdkBody).
    ///
    /// This method is available only to Rust and it is required to comply with the
    /// interface required by the code generator.
    pub fn new(body: aws_smithy_http::body::SdkBody) -> Self {
        Self(Arc::new(Mutex::new(
            aws_smithy_http::byte_stream::ByteStream::new(body),
        )))
    }
}

/// ByteStream Abstractions.
#[pymethods]
impl ByteStream {
    /// Create a new [ByteStream](aws_smithy_http::byte_stream::ByteStream) from a slice of bytes.
    #[new]
    pub fn newpy(input: &[u8]) -> Self {
        Self(Arc::new(Mutex::new(
            aws_smithy_http::byte_stream::ByteStream::new(aws_smithy_http::body::SdkBody::from(
                input,
            )),
        )))
    }

    /// Create a new [ByteStream](aws_smithy_http::byte_stream::ByteStream) from a path, without
    /// requiring Python to await this method.
    ///
    /// **NOTE:** This method will block the Rust event loop when it is running.
    #[staticmethod]
    pub fn from_path_blocking(py: Python, path: String) -> PyResult<Py<PyAny>> {
        let byte_stream = futures::executor::block_on(async {
            aws_smithy_http::byte_stream::ByteStream::from_path(path)
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))
        })?;
        let result = Self(Arc::new(Mutex::new(byte_stream)));
        Ok(result.into_py(py))
    }

    /// Create a new [ByteStream](aws_smithy_http::byte_stream::ByteStream) from a path, forcing
    /// Python to await this coroutine.
    #[staticmethod]
    pub fn from_path(py: Python, path: String) -> PyResult<&PyAny> {
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let byte_stream = aws_smithy_http::byte_stream::ByteStream::from_path(path)
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
            Ok(Self(Arc::new(Mutex::new(byte_stream))))
        })
    }

    /// Allow to syncronously iterate over the stream.
    ///
    /// More info: `<https://docs.python.org/3/reference/datamodel.html#object.__iter__.>`
    pub fn __iter__(slf: PyRef<Self>) -> PyRef<Self> {
        slf
    }

    /// Return the next item from the iterator. If there are no further items, raise the StopIteration exception.
    /// PyO3 allows to raise the correct exception using the enum [IterNextOutput](pyo3::pyclass::IterNextOutput).
    ///
    /// To get tnext value of the iterator, the `Arc` inner stream is cloned and the Rust call to `next()` is executed
    /// inside a call blocking the Tokio runtime.
    ///
    /// More info: `<https://docs.python.org/3/reference/datamodel.html#object.__next__.>`
    pub fn __next__(slf: PyRefMut<Self>) -> PyResult<IterNextOutput<Py<PyAny>, PyObject>> {
        let body = slf.0.clone();
        let data_chunk = futures::executor::block_on(yield_data_chunk(body));
        match data_chunk {
            Ok(data_chunk) => Ok(IterNextOutput::Yield(data_chunk.into_py(slf.py()))),
            Err(e) => {
                if e.is_instance_of::<PyStopIteration>(slf.py()) {
                    Ok(IterNextOutput::Return(slf.py().None()))
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Allow to asyncronously iterate over the stream.
    ///
    /// More info: `<https://docs.python.org/3/reference/datamodel.html#object.__aiter__.>`
    pub fn __aiter__(slf: PyRef<Self>) -> PyRef<Self> {
        slf
    }

    /// Return an awaitable resulting in a next value of the iterator or raise a StopAsyncIteration
    /// exception when the iteration is over. PyO3 allows to raise the correct exception using the enum
    /// [IterANextOutput](pyo3::pyclass::IterANextOutput).
    ///
    /// To get the next value of the iterator, the `Arc` inner stream is cloned and the Rust call
    /// to `next()` is converted into an awaitable Python coroutine.
    ///
    /// More info: `<https://docs.python.org/3/reference/datamodel.html#object.__anext__.>`
    pub fn __anext__(slf: PyRefMut<Self>) -> PyResult<IterANextOutput<Py<PyAny>, PyObject>> {
        let body = slf.0.clone();
        let data_chunk = pyo3_asyncio::tokio::local_future_into_py(slf.py(), async move {
            let data = yield_data_chunk(body).await?;
            Ok(Python::with_gil(|py| data.into_py(py)))
        });
        match data_chunk {
            Ok(data_chunk) => Ok(IterANextOutput::Yield(data_chunk.into_py(slf.py()))),
            Err(e) => {
                if e.is_instance_of::<PyStopIteration>(slf.py()) {
                    Ok(IterANextOutput::Return(slf.py().None()))
                } else {
                    Err(e)
                }
            }
        }
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

    // TODO(When running in sync python, the iterator is reading the whole content in memory. Figure out why.)
    #[tokio::test]
    async fn bytestream_can_be_used_in_sync_python_when_initialized_in_rust() -> PyResult<()> {
        crate::tests::initialize();
        Python::with_gil(|py| {
            let bytes = "repeat\n".repeat(100000);
            let bytestream = ByteStream::newpy(bytes.as_bytes());
            let bytestream = PyCell::new(py, bytestream).unwrap();
            py_run!(
                py,
                bytestream,
                r#"
                for chunk in bytestream:
                    assert len(chunk) > 10
            "#
            )
        });
        Ok(())
    }
}
