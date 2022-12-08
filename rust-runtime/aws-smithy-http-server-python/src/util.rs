/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

pub mod collection;
pub mod error;

use pyo3::prelude::*;

// Captures some information about a Python function.
#[derive(Debug, PartialEq)]
pub struct FuncMetadata {
    pub name: String,
    pub is_coroutine: bool,
    pub num_args: usize,
}

// Returns `FuncMetadata` for given `func`.
pub fn func_metadata(py: Python, func: &PyObject) -> PyResult<FuncMetadata> {
    let name = func.getattr(py, "__name__")?.extract::<String>(py)?;
    let is_coroutine = is_coroutine(py, func)?;
    let inspect = py.import("inspect")?;
    let args = inspect
        .call_method1("getargs", (func.getattr(py, "__code__")?,))?
        .getattr("args")?
        .extract::<Vec<String>>()?;
    Ok(FuncMetadata {
        name,
        is_coroutine,
        num_args: args.len(),
    })
}

// Check if a Python function is a coroutine. Since the function has not run yet,
// we cannot use `asyncio.iscoroutine()`, we need to use `inspect.iscoroutinefunction()`.
fn is_coroutine(py: Python, func: &PyObject) -> PyResult<bool> {
    let inspect = py.import("inspect")?;
    // NOTE: that `asyncio.iscoroutine()` doesn't work here.
    inspect
        .call_method1("iscoroutinefunction", (func,))?
        .extract::<bool>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_metadata() -> PyResult<()> {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let module = PyModule::from_code(
                py,
                r#"
def regular_func(first_arg, second_arg):
    pass

async def async_func():
    pass
"#,
                "",
                "",
            )?;

            let regular_func = module.getattr("regular_func")?.into_py(py);
            assert_eq!(
                FuncMetadata {
                    name: "regular_func".to_string(),
                    is_coroutine: false,
                    num_args: 2,
                },
                func_metadata(py, &regular_func)?
            );

            let async_func = module.getattr("async_func")?.into_py(py);
            assert_eq!(
                FuncMetadata {
                    name: "async_func".to_string(),
                    is_coroutine: true,
                    num_args: 0,
                },
                func_metadata(py, &async_func)?
            );

            Ok(())
        })
    }
}
