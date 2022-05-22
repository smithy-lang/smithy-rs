/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Python wrapped types from aws-smithy-types.

use pyo3::prelude::*;

///
#[pyclass]
#[derive(Debug, Clone, PartialEq)]
pub struct Blob(aws_smithy_types::Blob);

#[pymethods]
impl Blob {
    ///
    #[new]
    pub fn new(input: Vec<u8>) -> Self {
        Self(aws_smithy_types::Blob::new(input))
    }

    ///
    #[getter(data)]
    pub fn get_data(&self) -> &[u8] {
        self.0.as_ref()
    }

    ///
    #[setter(data)]
    pub fn set_data(&mut self, data: Vec<u8>) {
        *self = Self::new(data);
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
            let blob = Blob(aws_smithy_types::Blob::new("some data".as_bytes().to_vec()));
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
