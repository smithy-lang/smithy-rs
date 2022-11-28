/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Python context definition.

use std::collections::HashSet;

use pyo3::{types::PyDict, PyObject, PyResult, Python, ToPyObject};

use crate::{rich_py_err, util::is_optional_of};

use super::lambda::PyLambdaContext;

pub mod layer;
#[cfg(test)]
mod testing;

/// PyContext is a wrapper for context object provided by the user.
/// It injects some values (currently only [PyLambdaContext]) that is type-hinted by the user.
///
///
/// PyContext is initialised during the startup, it inspects the provided context object for fields
/// that are type-hinted to inject some values provided by the framework (see [PyContext::new()]).
///
/// After finding fields that needs to be injected, [layer::AddPyContextLayer], a [tower::Layer],
/// populates request-scoped values from incoming request.
///
/// And finally PyContext implements [ToPyObject] (so it can by passed to Python handlers)
/// that provides [PyObject] provided by the user with the additional values injected by the framework.
#[derive(Clone)]
pub struct PyContext {
    inner: PyObject,
    // TODO(Refactor): We should ideally keep record of injectable fields in a hashmap like:
    // `injectable_fields: HashMap<Field, Box<dyn Injectable>>` where `Injectable` provides a method to extract a `PyObject` from a `Request`,
    // but I couldn't find a way to extract a trait object from a Python object.
    // We could introduce a registry to keep track of every injectable type but I'm not sure that is the best way to do it,
    // so until we found a good way to achive that, I didn't want to introduce any abstraction here and
    // keep it simple because we only have one field that is injectable.
    lambda_ctx_fields: HashSet<String>,
}

impl PyContext {
    pub fn new(inner: PyObject) -> PyResult<Self> {
        let lambda_ctx_fields = Python::with_gil(|py| get_lambda_ctx_fields(py, &inner))?;
        Ok(Self {
            inner,
            lambda_ctx_fields,
        })
    }

    /// Returns true if custom context class provided by the user injects [PyLambdaContext].
    pub fn has_lambda_context_fields(&self) -> bool {
        !self.lambda_ctx_fields.is_empty()
    }

    /// Sets given `lambda_ctx` to user provided context class.
    pub fn set_lambda_context(&self, lambda_ctx: Option<PyLambdaContext>) {
        if !self.has_lambda_context_fields() {
            // Return early without acquiring GIL
            return;
        }

        let inner = &self.inner;
        Python::with_gil(|py| {
            for field in self.lambda_ctx_fields.iter() {
                if let Err(err) = inner.setattr(py, field.as_str(), lambda_ctx.clone()) {
                    tracing::warn!(field = ?field, error = ?rich_py_err(err), "could not inject `LambdaContext` to context")
                }
            }
        });
    }
}

impl ToPyObject for PyContext {
    fn to_object(&self, _py: Python<'_>) -> PyObject {
        self.inner.clone()
    }
}

// Inspects the given `PyObject` to detect fields that type-hinted `PyLambdaContext`.
fn get_lambda_ctx_fields(py: Python, ctx: &PyObject) -> PyResult<HashSet<String>> {
    let typing = py.import("typing")?;
    let hints = match typing
        .call_method1("get_type_hints", (ctx,))
        .and_then(|res| res.extract::<&PyDict>())
    {
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
        if is_optional_of::<PyLambdaContext>(py, value)? {
            fields.insert(key.to_string());
        }
    }
    Ok(fields)
}

#[cfg(test)]
mod tests {
    use pyo3::{prelude::*, py_run};

    use crate::context::testing::{get_context, py_lambda_ctx};

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

        ctx.set_lambda_context(Some(py_lambda_ctx("my-req-id", "123")));
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
        ctx.set_lambda_context(None);
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
        ctx.set_lambda_context(Some(py_lambda_ctx("my-req-id", "123")));
        Python::with_gil(|py| {
            py_run!(py, ctx, "assert ctx is None");
        });

        Ok(())
    }
}
