/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Python wrapped types from aws-smithy-types.
//!
//! ## `Deref` hacks for Json serializer
//! [aws_smithy_json::serialize::JsonValueWriter] expects references to the types
//! from [aws_smithy_types] (for example [aws_smithy_json::serialize::JsonValueWriter::document()]
//! expects `&aws_smithy_types::Document`). In order to make
//! [aws_smithy_json::serialize::JsonValueWriter] happy, we implement `Deref` traits for
//! Python types to their Rust counterparts (for example
//! `impl Deref<Target=aws_smithy_types::Document> for Document` and that allows `&Document` to
//! get coerced to `&aws_smithy_types::Document`). This is a hack, we should ideally handle this
//! in `JsonSerializerGenerator.kt` but it's not easy to do it with our current Kotlin structure.

use std::{
    collections::HashMap,
    future::Future,
    ops::Deref,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use bytes::Bytes;
use pyo3::{
    exceptions::{PyRuntimeError, PyTypeError},
    prelude::*,
    types::IntoPyDict,
    IntoPyObjectExt,
};
use tokio::{runtime::Handle, sync::Mutex};

use crate::PyError;

/// Python Wrapper for [aws_smithy_types::Blob].
///
/// :param input bytes:
/// :rtype None:
#[pyclass]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

#[allow(non_local_definitions)]
#[pymethods]
impl Blob {
    /// Create a new Python instance of `Blob`.
    #[new]
    pub fn pynew(input: Vec<u8>) -> Self {
        Self(aws_smithy_types::Blob::new(input))
    }

    /// Python getter for the `Blob` byte array.
    ///
    /// :type bytes:
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

impl AsRef<[u8]> for Blob {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
    ///
    /// :param epoch_seconds int:
    /// :rtype DateTime:
    #[staticmethod]
    pub fn from_secs(epoch_seconds: i64) -> Self {
        Self(aws_smithy_types::date_time::DateTime::from_secs(
            epoch_seconds,
        ))
    }

    /// Creates a `DateTime` from a number of milliseconds since the Unix epoch.
    ///
    /// :param epoch_millis int:
    /// :rtype DateTime:
    #[staticmethod]
    pub fn from_millis(epoch_millis: i64) -> Self {
        Self(aws_smithy_types::date_time::DateTime::from_secs(
            epoch_millis,
        ))
    }

    /// Creates a `DateTime` from a number of nanoseconds since the Unix epoch.
    ///
    /// :param epoch_nanos int:
    /// :rtype DateTime:
    #[staticmethod]
    pub fn from_nanos(epoch_nanos: i128) -> PyResult<Self> {
        Ok(Self(
            aws_smithy_types::date_time::DateTime::from_nanos(epoch_nanos)
                .map_err(PyError::DateTimeConversion)?,
        ))
    }

    /// Read 1 date of `format` from `s`, expecting either `delim` or EOF.
    ///
    /// TODO(PythonTyping): How do we represent `char` in Python?
    ///
    /// :param format Format:
    /// :param delim str:
    /// :rtype typing.Tuple[DateTime, str]:
    #[staticmethod]
    pub fn read(s: &str, format: Format, delim: char) -> PyResult<(Self, &str)> {
        let (self_, next) = aws_smithy_types::date_time::DateTime::read(s, format.into(), delim)
            .map_err(PyError::DateTimeParse)?;
        Ok((Self(self_), next))
    }

    /// Creates a `DateTime` from a number of seconds and a fractional second since the Unix epoch.
    ///
    /// :param epoch_seconds int:
    /// :param fraction float:
    /// :rtype DateTime:
    #[staticmethod]
    pub fn from_fractional_secs(epoch_seconds: i64, fraction: f64) -> Self {
        Self(aws_smithy_types::date_time::DateTime::from_fractional_secs(
            epoch_seconds,
            fraction,
        ))
    }

    /// Creates a `DateTime` from a number of seconds and sub-second nanos since the Unix epoch.
    ///
    /// :param seconds int:
    /// :param subsecond_nanos int:
    /// :rtype DateTime:
    #[staticmethod]
    pub fn from_secs_and_nanos(seconds: i64, subsecond_nanos: u32) -> Self {
        Self(aws_smithy_types::date_time::DateTime::from_secs_and_nanos(
            seconds,
            subsecond_nanos,
        ))
    }

    /// Creates a `DateTime` from an `f64` representing the number of seconds since the Unix epoch.
    ///
    /// :param epoch_seconds float:
    /// :rtype DateTime:
    #[staticmethod]
    pub fn from_secs_f64(epoch_seconds: f64) -> Self {
        Self(aws_smithy_types::date_time::DateTime::from_secs_f64(
            epoch_seconds,
        ))
    }

    /// Parses a `DateTime` from a string using the given `format`.
    ///
    /// :param s str:
    /// :param format Format:
    /// :rtype DateTime:
    #[staticmethod]
    pub fn from_str(s: &str, format: Format) -> PyResult<Self> {
        Ok(Self(
            aws_smithy_types::date_time::DateTime::from_str(s, format.into())
                .map_err(PyError::DateTimeParse)?,
        ))
    }

    /// Returns the number of nanoseconds since the Unix epoch that this `DateTime` represents.
    ///
    /// :rtype int:
    pub fn as_nanos(&self) -> i128 {
        self.0.as_nanos()
    }

    /// Returns the `DateTime` value as an `f64` representing the seconds since the Unix epoch.
    ///
    /// :rtype float:
    pub fn as_secs_f64(&self) -> f64 {
        self.0.as_secs_f64()
    }

    /// Returns true if sub-second nanos is greater than zero.
    ///
    /// :rtype bool:
    pub fn has_subsec_nanos(&self) -> bool {
        self.0.has_subsec_nanos()
    }

    /// Returns the epoch seconds component of the `DateTime`.
    ///
    /// :rtype int:
    pub fn secs(&self) -> i64 {
        self.0.secs()
    }

    /// Returns the sub-second nanos component of the `DateTime`.
    ///
    /// :rtype int:
    pub fn subsec_nanos(&self) -> u32 {
        self.0.subsec_nanos()
    }

    /// Converts the `DateTime` to the number of milliseconds since the Unix epoch.
    ///
    /// :rtype int:
    pub fn to_millis(&self) -> PyResult<i64> {
        Ok(self.0.to_millis().map_err(PyError::DateTimeConversion)?)
    }
}

impl From<aws_smithy_types::DateTime> for DateTime {
    fn from(other: aws_smithy_types::DateTime) -> DateTime {
        DateTime(other)
    }
}

impl Deref for DateTime {
    type Target = aws_smithy_types::DateTime;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Python Wrapper for [aws_smithy_types::byte_stream::ByteStream].
///
/// ByteStream provides misuse-resistant primitives to make it easier to handle common patterns with streaming data.
///
/// On the Rust side, The Python implementation wraps the original [ByteStream](aws_smithy_types::byte_stream::ByteStream)
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
/// The original Rust [ByteStream](aws_smithy_types::byte_stream::ByteStream) is wrapped inside a `Arc<Mutex>` to allow the type to be
/// [Clone] (required by PyO3) and to allow internal mutability, required to fetch the next chunk of data.
///
/// :param input bytes:
/// :rtype None:
#[pyclass]
#[derive(Debug, Clone)]
pub struct ByteStream(Arc<Mutex<aws_smithy_types::byte_stream::ByteStream>>);

impl futures::stream::Stream for ByteStream {
    type Item = Result<Bytes, aws_smithy_types::byte_stream::error::Error>;

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
    body: Arc<Mutex<aws_smithy_types::byte_stream::ByteStream>>,
) -> PyResult<Option<Bytes>> {
    let mut stream = body.lock().await;
    stream
        .next()
        .await
        .transpose()
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))
}

impl ByteStream {
    /// Construct a new [`ByteStream`](aws_smithy_types::byte_stream::ByteStream) from a
    /// [`SdkBody`](aws_smithy_types::body::SdkBody).
    ///
    /// This method is available only to Rust and it is required to comply with the
    /// interface required by the code generator.
    pub fn new(body: aws_smithy_types::body::SdkBody) -> Self {
        Self(Arc::new(Mutex::new(
            aws_smithy_types::byte_stream::ByteStream::new(body),
        )))
    }
}

impl Default for ByteStream {
    fn default() -> Self {
        Self::new(aws_smithy_types::body::SdkBody::from(""))
    }
}

#[pymethods]
impl ByteStream {
    /// Create a new [ByteStream](aws_smithy_types::byte_stream::ByteStream) from a slice of bytes.
    #[new]
    pub fn newpy(input: &[u8]) -> Self {
        Self(Arc::new(Mutex::new(
            aws_smithy_types::byte_stream::ByteStream::new(aws_smithy_types::body::SdkBody::from(
                input,
            )),
        )))
    }

    /// Create a new [ByteStream](aws_smithy_types::byte_stream::ByteStream) from a path, without
    /// requiring Python to await this method.
    ///
    /// **NOTE:** This method will block the Rust event loop when it is running.
    ///
    /// :param path str:
    /// :rtype ByteStream:
    #[staticmethod]
    pub fn from_path_blocking(py: Python, path: String) -> PyResult<PyObject> {
        let byte_stream = Handle::current().block_on(async {
            aws_smithy_types::byte_stream::ByteStream::from_path(path)
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))
        })?;
        let result = Self(Arc::new(Mutex::new(byte_stream)));
        result.into_py_any(py)
    }

    /// Create a new [ByteStream](aws_smithy_types::byte_stream::ByteStream) from a path, forcing
    /// Python to await this coroutine.
    ///
    /// :param path str:
    /// :rtype typing.Awaitable[ByteStream]:
    #[staticmethod]
    pub fn from_path(py: Python, path: String) -> PyResult<Bound<'_, PyAny>> {
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let byte_stream = aws_smithy_types::byte_stream::ByteStream::from_path(path)
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
    /// To get the next value of the iterator, the `Arc` inner stream is cloned and the Rust call to `next()` is executed
    /// inside a call blocking the Tokio runtime.
    ///
    /// More info: `<https://docs.python.org/3/reference/datamodel.html#object.__next__.>`
    pub fn __next__(slf: PyRefMut<Self>) -> PyResult<Option<PyObject>> {
        let body = slf.0.clone();
        let data_chunk = futures::executor::block_on(yield_data_chunk(body));
        match data_chunk {
            Ok(Some(data_chunk)) => {
                let object = data_chunk.into_pyobject(slf.py())?.unbind();
                Ok(Some(object))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Allow to asyncronously iterate over the stream.
    ///
    /// More info: `<https://docs.python.org/3/reference/datamodel.html#object.__aiter__.>`
    pub fn __aiter__(slf: PyRef<Self>) -> PyRef<Self> {
        slf
    }

    /// Return an awaitable resulting in a next value of the iterator or raise a StopAsyncIteration
    /// exception when the iteration is over.
    ///
    /// To get the next value of the iterator, the `Arc` inner stream is cloned and the Rust call
    /// to `next()` is converted into an awaitable Python coroutine.
    ///
    /// More info: `<https://docs.python.org/3/reference/datamodel.html#object.__anext__.>`
    ///
    /// About the return type, we cannot use `IterANextOutput` because we don't know if we
    /// have a next value or not until we call the `next` on the underlying stream which is
    /// an async operation and it's awaited on the Python side. So we're returning
    /// `StopAsyncIteration` inside the returned future lazily.
    /// The reason for the extra `Option` wrapper is that PyO3 expects `__anext__` to return
    /// either `Option<PyObject>` or `IterANextOutput` and fails to compile otherwise, so we're
    /// using extra `Option` just to make PyO3 happy.
    pub fn __anext__(slf: PyRefMut<Self>) -> PyResult<Option<PyObject>> {
        let body = slf.0.clone();
        let fut = pyo3_async_runtimes::tokio::future_into_py(slf.py(), async move {
            let data = yield_data_chunk(body).await?;
            match data {
                Some(data) => Python::with_gil(|py| {
                    let py_obj = data.into_pyobject(py)?;
                    Ok(Some(py_obj.unbind()))
                }),
                None => Ok(None),
            }
        })?;
        Ok(Some(fut.into()))
    }
}

/// Python Wrapper for [aws_smithy_types::Document].
#[derive(Debug, Clone, PartialEq)]
pub struct Document(aws_smithy_types::Document);

impl<'py> IntoPyObject<'py> for Document {
    type Target = PyAny;
    type Output = Bound<'py, PyAny>;
    type Error = PyErr;

    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        use aws_smithy_types::{Document as D, Number};

        match self.0 {
            D::Object(obj) => {
                let dict: HashMap<_, _> = obj
                    .into_iter()
                    .map(|(k, v)| Document(v).into_py_any(py).map(|py_obj| (k, py_obj)))
                    .collect::<Result<HashMap<_, _>, _>>()?;
                dict.into_py_dict(py).map(|d| d.into_any())
            }

            D::Array(vec) => {
                let py_vec: Vec<_> = vec
                    .into_iter()
                    .map(|d| Document(d).into_pyobject(py))
                    .collect::<Result<Vec<_>, _>>()?;
                py_vec.into_pyobject(py)
            }
            D::Number(Number::Float(f)) => Ok(f.into_pyobject(py)?.into_any()),
            D::Number(Number::PosInt(pi)) => Ok(pi.into_pyobject(py)?.into_any()),
            D::Number(Number::NegInt(ni)) => Ok(ni.into_pyobject(py)?.into_any()),
            D::String(str) => Ok(str.into_pyobject(py)?.into_any()),
            D::Bool(bool) => Ok(bool.into_pyobject(py)?.to_owned().into_any()),
            D::Null => Ok(py.None().into_pyobject(py)?.into_any()),
        }
    }
}

impl FromPyObject<'_> for Document {
    fn extract_bound(obj: &Bound<'_, PyAny>) -> PyResult<Self> {
        use aws_smithy_types::{Document as D, Number};

        if let Ok(obj) = obj.extract::<HashMap<String, Document>>() {
            Ok(Self(D::Object(
                obj.into_iter().map(|(k, v)| (k, v.0)).collect(),
            )))
        } else if let Ok(vec) = obj.extract::<Vec<Self>>() {
            Ok(Self(D::Array(vec.into_iter().map(|d| d.0).collect())))
        } else if let Ok(b) = obj.extract::<bool>() {
            // This check must happen before any number checks because they cast
            // `true`, `false` to `1`, `0` respectively.
            Ok(Self(D::Bool(b)))
        } else if let Ok(pi) = obj.extract::<u64>() {
            Ok(Self(D::Number(Number::PosInt(pi))))
        } else if let Ok(ni) = obj.extract::<i64>() {
            Ok(Self(D::Number(Number::NegInt(ni))))
        } else if let Ok(f) = obj.extract::<f64>() {
            Ok(Self(D::Number(Number::Float(f))))
        } else if let Ok(s) = obj.extract::<String>() {
            Ok(Self(D::String(s)))
        } else if obj.is_none() {
            Ok(Self(D::Null))
        } else {
            Err(PyTypeError::new_err(format!(
                "'{obj}' cannot be converted to 'Document'",
            )))
        }
    }
}

impl Deref for Document {
    type Target = aws_smithy_types::Document;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<aws_smithy_types::Document> for Document {
    fn from(other: aws_smithy_types::Document) -> Document {
        Document(other)
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
            let blob = Bound::new(py, blob).unwrap();
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
            let datetime = Bound::new(py, datetime).unwrap();
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
            let bytestream = Bound::new(py, bytestream).unwrap();
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

    #[test]
    fn document_type() {
        use aws_smithy_types::{Document as D, Number};

        crate::tests::initialize();

        let cases = [
            (D::Null, c"None"),
            (D::Bool(true), c"True"),
            (D::Bool(false), c"False"),
            (D::String("foobar".to_string()), c"'foobar'"),
            (D::Number(Number::Float(42.0)), c"42.0"),
            (D::Number(Number::PosInt(142)), c"142"),
            (D::Number(Number::NegInt(-152)), c"-152"),
            (
                D::Array(vec![
                    D::Bool(false),
                    D::String("qux".to_string()),
                    D::Number(Number::Float(1.0)),
                    D::Array(vec![D::String("inner".to_string()), D::Bool(true)]),
                ]),
                c"[False, 'qux', 1.0, ['inner', True]]",
            ),
            (
                D::Object(
                    [
                        ("t".to_string(), D::Bool(true)),
                        ("foo".to_string(), D::String("foo".to_string())),
                        ("f42".to_string(), D::Number(Number::Float(42.0))),
                        ("i42".to_string(), D::Number(Number::PosInt(42))),
                        ("f".to_string(), D::Bool(false)),
                        (
                            "vec".to_string(),
                            D::Array(vec![
                                D::String("inner".to_string()),
                                D::Object(
                                    [
                                        (
                                            "nested".to_string(),
                                            D::String("nested_value".to_string()),
                                        ),
                                        ("nested_num".to_string(), D::Number(Number::NegInt(-42))),
                                    ]
                                    .into(),
                                ),
                            ]),
                        ),
                    ]
                    .into(),
                ),
                c"{
                    't': True,
                    'foo': 'foo',
                    'f42': 42.0,
                    'i42': 42,
                    'f': False,
                    'vec': [
                        'inner',
                        {'nested': 'nested_value', 'nested_num': -42}
                    ]
                }",
            ),
        ];

        for (rust_ty, python_repr) in cases {
            // Rust -> Python
            Python::with_gil(|py| {
                let value = Document(rust_ty.clone())
                    .into_pyobject(py)
                    .expect("conversion failed");
                let python_code = format!(
                    "assert value == {}",
                    python_repr.to_str().expect("invalid UTF-8 in CStr")
                );
                py_run!(py, value, &python_code);
            });

            // Python -> Rust
            Python::with_gil(|py| {
                let py_value = py.eval(python_repr, None, None).unwrap();
                let doc = py_value.extract::<Document>().unwrap();
                assert_eq!(doc, Document(rust_ty.clone()));
            });

            // Rust -> Python -> Rust
            Python::with_gil(|py| {
                let doc = Document(rust_ty);
                let doc2 = doc
                    .clone()
                    .into_pyobject(py)
                    .expect("conversion failed")
                    .extract()
                    .unwrap();
                assert_eq!(doc, doc2);
            });
        }
    }
}
