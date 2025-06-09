/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{mem, str::FromStr, sync::Arc};

use http::{header::HeaderName, HeaderMap, HeaderValue};
use parking_lot::Mutex;
use pyo3::{
    exceptions::{PyKeyError, PyValueError},
    pyclass, PyErr, PyResult,
};

use crate::{mutable_mapping_pymethods, util::collection::PyMutableMapping};

/// Python-compatible [HeaderMap] object.
#[pyclass(mapping)]
#[derive(Clone, Debug)]
pub struct PyHeaderMap {
    inner: Arc<Mutex<HeaderMap>>,
}

impl PyHeaderMap {
    pub fn new(inner: HeaderMap) -> Self {
        Self {
            inner: Arc::new(Mutex::new(inner)),
        }
    }

    // Consumes self by taking the inner `HeaderMap`.
    // This method would have been `into_inner(self) -> HeaderMap`
    // but we can't do that because we are crossing Python boundary.
    pub fn take_inner(&mut self) -> Option<HeaderMap> {
        let header_map = mem::take(&mut self.inner);
        let header_map = Arc::try_unwrap(header_map).ok()?;
        let header_map = header_map.into_inner();
        Some(header_map)
    }
}

/// By implementing [PyMutableMapping] for [PyHeaderMap] we are making it to
/// behave like a dictionary on the Python.
impl PyMutableMapping for PyHeaderMap {
    type Key = String;
    type Value = String;

    fn len(&self) -> PyResult<usize> {
        Ok(self.inner.lock().len())
    }

    fn contains(&self, key: Self::Key) -> PyResult<bool> {
        Ok(self.inner.lock().contains_key(key))
    }

    fn keys(&self) -> PyResult<Vec<Self::Key>> {
        Ok(self.inner.lock().keys().map(|h| h.to_string()).collect())
    }

    fn values(&self) -> PyResult<Vec<Self::Value>> {
        self.inner
            .lock()
            .values()
            .map(|h| h.to_str().map(|s| s.to_string()).map_err(to_value_error))
            .collect()
    }

    fn get(&self, key: Self::Key) -> PyResult<Option<Self::Value>> {
        self.inner
            .lock()
            .get(key)
            .map(|h| h.to_str().map(|s| s.to_string()).map_err(to_value_error))
            .transpose()
    }

    fn set(&mut self, key: Self::Key, value: Self::Value) -> PyResult<()> {
        self.inner.lock().insert(
            HeaderName::from_str(&key).map_err(to_value_error)?,
            HeaderValue::from_str(&value).map_err(to_value_error)?,
        );
        Ok(())
    }

    fn del(&mut self, key: Self::Key) -> PyResult<()> {
        if self.inner.lock().remove(key).is_none() {
            Err(PyKeyError::new_err("unknown key"))
        } else {
            Ok(())
        }
    }
}

mutable_mapping_pymethods!(PyHeaderMap, keys_iter: PyHeaderMapKeys);

fn to_value_error(err: impl std::error::Error) -> PyErr {
    PyValueError::new_err(err.to_string())
}

#[cfg(test)]
mod tests {
    use http::header;
    use pyo3::{prelude::*, py_run};

    use super::*;

    #[test]
    fn py_header_map() -> PyResult<()> {
        pyo3::prepare_freethreaded_python();

        let mut header_map = HeaderMap::new();
        header_map.insert(header::CONTENT_LENGTH, "42".parse().unwrap());
        header_map.insert(header::HOST, "localhost".parse().unwrap());

        let header_map = Python::with_gil(|py| {
            let py_header_map = PyHeaderMap::new(header_map);
            let headers = Bound::new(py, py_header_map)?;
            py_run!(
                py,
                headers,
                r#"
assert len(headers) == 2
assert headers["content-length"] == "42"
assert headers["host"] == "localhost"

headers["content-length"] = "45"
assert headers["content-length"] == "45"
headers["content-encoding"] = "application/json"
assert headers["content-encoding"] == "application/json"

del headers["host"]
assert headers.get("host") == None
assert len(headers) == 2

assert set(headers.items()) == set([
    ("content-length", "45"),
    ("content-encoding", "application/json")
])
"#
            );

            Ok::<_, PyErr>(headers.borrow_mut().take_inner().unwrap())
        })?;

        assert_eq!(
            header_map,
            vec![
                (header::CONTENT_LENGTH, "45".parse().unwrap()),
                (
                    header::CONTENT_ENCODING,
                    "application/json".parse().unwrap()
                ),
            ]
            .into_iter()
            .collect()
        );

        Ok(())
    }
}
