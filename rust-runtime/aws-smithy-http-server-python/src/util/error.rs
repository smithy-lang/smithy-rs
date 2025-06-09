/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Provides utilities for Python errors.

use std::fmt;

use pyo3::{types::PyTracebackMethods, PyErr, Python};

/// Wraps [PyErr] with a richer debug output that includes traceback and cause.
pub struct RichPyErr(PyErr);

impl fmt::Debug for RichPyErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        Python::with_gil(|py| {
            let mut debug_struct = f.debug_struct("RichPyErr");
            debug_struct
                .field("type", self.0.get_type(py).as_any())
                .field("value", self.0.value(py));

            if let Some(traceback) = self.0.traceback(py) {
                if let Ok(traceback) = traceback.format() {
                    debug_struct.field("traceback", &traceback);
                }
            }

            if let Some(cause) = self.0.cause(py) {
                debug_struct.field("cause", &rich_py_err(cause));
            }

            debug_struct.finish()
        })
    }
}

/// Wrap `err` with [RichPyErr] to have a richer debug output.
pub fn rich_py_err(err: PyErr) -> RichPyErr {
    RichPyErr(err)
}

#[cfg(test)]
mod tests {
    use pyo3::prelude::*;

    use super::*;

    #[test]
    fn rich_python_errors() -> PyResult<()> {
        pyo3::prepare_freethreaded_python();

        let py_err = Python::with_gil(|py| {
            py.run(
                cr#"
def foo():
    base_err = ValueError("base error")
    raise ValueError("some python error") from base_err

def bar():
    foo()

def baz():
    bar()

baz()
"#,
                None,
                None,
            )
            .unwrap_err()
        });

        let debug_output = format!("{:?}", rich_py_err(py_err));

        // Make sure we are capturing error message
        assert!(debug_output.contains("some python error"));

        // Make sure we are capturing traceback
        assert!(debug_output.contains("foo"));
        assert!(debug_output.contains("bar"));
        assert!(debug_output.contains("baz"));

        // Make sure we are capturing cause
        assert!(debug_output.contains("base error"));

        Ok(())
    }
}
