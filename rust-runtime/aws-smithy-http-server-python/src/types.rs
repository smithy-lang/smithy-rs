/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Python wrapped types from aws-smithy-types.

use pyo3::prelude::*;

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
}
