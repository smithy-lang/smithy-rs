/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Support for injecting [PyLambdaContext] to [super::PyContext].

use std::collections::HashSet;

use http::Extensions;
use lambda_http::Context as LambdaContext;
use pyo3::{
    types::{PyAnyMethods, PyDict},
    PyObject, PyResult, Python,
};

use crate::{lambda::PyLambdaContext, rich_py_err, util::is_optional_of};

#[derive(Clone)]
pub struct PyContextLambda {
    fields: HashSet<String>,
}

impl PyContextLambda {
    pub fn new(ctx: PyObject) -> PyResult<Self> {
        let fields = Python::with_gil(|py| get_lambda_ctx_fields(py, &ctx))?;
        Ok(Self { fields })
    }

    pub fn populate_from_extensions(&self, ctx: PyObject, ext: &Extensions) {
        if self.fields.is_empty() {
            // Return early without acquiring GIL
            return;
        }

        let lambda_ctx = ext
            .get::<LambdaContext>()
            .cloned()
            .map(PyLambdaContext::new);

        Python::with_gil(|py| {
            for field in self.fields.iter() {
                if let Err(err) = ctx.setattr(py, field.as_str(), lambda_ctx.clone()) {
                    tracing::warn!(field = ?field, error = ?rich_py_err(err), "could not inject `LambdaContext` to context")
                }
            }
        });
    }
}

// Inspects the given `PyObject` to detect fields that type-hinted `PyLambdaContext`.
fn get_lambda_ctx_fields(py: Python, ctx: &PyObject) -> PyResult<HashSet<String>> {
    let typing = py.import("typing")?;
    let results = typing.call_method1("get_type_hints", (ctx,))?;
    let hints = match results.downcast::<PyDict>() {
        Ok(hints) => hints,
        Err(_) => {
            // `get_type_hints` could fail if `ctx` is `None`, which is the default value
            // for the context if user does not provide a custom class.
            // In that case, this is not really an error and we should just return an empty set.
            return Ok(HashSet::new());
        }
    };

    let mut fields = HashSet::new();
    for (key, value) in hints {
        if is_optional_of::<PyLambdaContext>(value.as_any())? {
            fields.insert(key.to_string());
        }
    }
    Ok(fields)
}

#[cfg(test)]
mod tests {
    use http::Extensions;
    use lambda_http::Context as LambdaContext;
    use pyo3::{prelude::*, py_run};

    use crate::context::testing::{get_context, lambda_ctx};

    #[test]
    fn py_context_with_lambda_context() -> PyResult<()> {
        pyo3::prepare_freethreaded_python();

        let ctx = get_context(
            r#"
class Context:
    foo: int = 0
    bar: str = 'qux'
    lambda_ctx: typing.Optional[LambdaContext]

ctx = Context()
ctx.foo = 42
"#,
        );
        Python::with_gil(|py| {
            py_run!(
                py,
                ctx,
                r#"
assert ctx.foo == 42
assert ctx.bar == 'qux'
assert not hasattr(ctx, 'lambda_ctx')
"#
            );
        });

        ctx.populate_from_extensions(&extensions_with_lambda_ctx(lambda_ctx("my-req-id", "123")));
        Python::with_gil(|py| {
            py_run!(
                py,
                ctx,
                r#"
assert ctx.lambda_ctx.request_id == "my-req-id"
assert ctx.lambda_ctx.deadline == 123
# Make some modifications
ctx.foo += 1
ctx.bar = 'baz'
"#
            );
        });

        // Assume we are getting a new request but that one doesn't have a `LambdaContext`,
        // in that case we should make fields `None` and shouldn't leak the previous `LambdaContext`.
        ctx.populate_from_extensions(&empty_extensions());
        Python::with_gil(|py| {
            py_run!(
                py,
                ctx,
                r#"
assert ctx.lambda_ctx is None
# Make sure we are preserving any modifications
assert ctx.foo == 43
assert ctx.bar == 'baz'
"#
            );
        });

        Ok(())
    }

    #[test]
    fn works_with_none() -> PyResult<()> {
        // Users can set context to `None` by explicity or implicitly by not providing a custom context class,
        // it shouldn't be fail in that case.

        pyo3::prepare_freethreaded_python();

        let ctx = get_context("ctx = None");
        ctx.populate_from_extensions(&extensions_with_lambda_ctx(lambda_ctx("my-req-id", "123")));
        Python::with_gil(|py| {
            py_run!(py, ctx, "assert ctx is None");
        });

        Ok(())
    }

    fn extensions_with_lambda_ctx(ctx: LambdaContext) -> Extensions {
        let mut exts = empty_extensions();
        exts.insert(ctx);
        exts
    }

    fn empty_extensions() -> Extensions {
        Extensions::new()
    }
}
