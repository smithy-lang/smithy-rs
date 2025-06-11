/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

pub mod collection;
pub mod error;

use pyo3::{PyAny, PyObject, PyResult, PyTypeInfo, Python};

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

// Checks whether given Python type is `Optional[T]`.
pub fn is_optional_of<T: PyTypeInfo>(py: Python, ty: &PyAny) -> PyResult<bool> {
    // for reference: https://stackoverflow.com/a/56833826

    // in Python `Optional[T]` is an alias for `Union[T, None]`
    // so we should check if the type origin is `Union`
    let union_ty = py.import("typing")?.getattr("Union")?;
    match ty.getattr("__origin__").map(|origin| origin.is(union_ty)) {
        Ok(true) => {}
        // Here we can ignore errors because `__origin__` is not present on all types
        // and it is not really an error, it is just a type we don't expect
        _ => return Ok(false),
    };

    let none = py.None();
    // in typing, `None` is a special case and it is converted to `type(None)`,
    // so we are getting type of `None` here to match
    let none_ty = none.as_ref(py).get_type();
    let target_ty = py.get_type::<T>();

    // `Union` should be tuple of `(T, NoneType)` or `(NoneType, T)`
    match ty
        .getattr("__args__")
        .and_then(|args| args.extract::<(&PyAny, &PyAny)>())
    {
        Ok((first_ty, second_ty)) => Ok(
            // (T, NoneType)
            (first_ty.is(target_ty) && second_ty.is(none_ty)) ||
                // (NoneType, T)
                (first_ty.is(none_ty) && second_ty.is(target_ty)),
        ),
        // Here we can ignore errors because `__args__` is not present on all types
        // and it is not really an error, it is just a type we don't expect
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use pyo3::{
        types::{PyBool, PyDict, PyModule, PyString},
        IntoPy,
    };

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

    #[allow(clippy::bool_assert_comparison)]
    #[test]
    fn check_if_is_optional_of() -> PyResult<()> {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let typing = py.import("typing")?;
            let module = PyModule::from_code(
                py,
                r#"
import typing

class Types:
    opt_of_str: typing.Optional[str] = "hello"
    opt_of_bool: typing.Optional[bool] = None
    regular_str: str = "world"
"#,
                "",
                "",
            )?;

            let types = module.getattr("Types")?.into_py(py);
            let type_hints = typing
                .call_method1("get_type_hints", (types,))
                .and_then(|res| res.extract::<&PyDict>())?;

            assert_eq!(
                true,
                is_optional_of::<PyString>(
                    py,
                    type_hints
                        .get_item("opt_of_str")
                        .expect("Python exception occurred during dictionary lookup")
                        .unwrap()
                )?
            );
            assert_eq!(
                false,
                is_optional_of::<PyString>(
                    py,
                    type_hints
                        .get_item("regular_str")
                        .expect("Python exception occurred during dictionary lookup")
                        .unwrap()
                )?
            );
            assert_eq!(
                true,
                is_optional_of::<PyBool>(
                    py,
                    type_hints
                        .get_item("opt_of_bool")
                        .expect("Python exception occurred during dictionary lookup")
                        .unwrap()
                )?
            );
            assert_eq!(
                false,
                is_optional_of::<PyString>(
                    py,
                    type_hints
                        .get_item("opt_of_bool")
                        .expect("Python exception occurred during dictionary lookup")
                        .unwrap()
                )?
            );

            Ok(())
        })
    }
}
