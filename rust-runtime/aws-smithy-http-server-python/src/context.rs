/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Python context definition.

use http::Extensions;
use pyo3::{Bound, BoundObject, IntoPyObject, PyAny, PyErr, PyObject, PyResult, Python};

mod lambda;
pub mod layer;
#[cfg(test)]
mod testing;

/// PyContext is a wrapper for context object provided by the user.
/// It injects some values (currently only [super::lambda::PyLambdaContext]) that is type-hinted by the user.
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
    lambda_ctx: lambda::PyContextLambda,
}

impl PyContext {
    pub fn new(inner: PyObject) -> PyResult<Self> {
        Ok(Self {
            lambda_ctx: lambda::PyContextLambda::new(inner.clone())?,
            inner,
        })
    }

    pub fn populate_from_extensions(&self, _ext: &Extensions) {
        self.lambda_ctx
            .populate_from_extensions(self.inner.clone(), _ext);
    }
}

impl<'py> IntoPyObject<'py> for PyContext {
    type Target = PyAny;
    type Error = PyErr;
    type Output = Bound<'py, Self::Target>;

    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        Ok(self.inner.bind(py).clone())
    }
}

#[cfg(test)]
mod tests {
    use http::Extensions;
    use pyo3::{prelude::*, py_run};

    use super::testing::get_context;

    #[test]
    fn py_context() -> PyResult<()> {
        pyo3::prepare_freethreaded_python();

        let ctx = get_context(
            r#"
class Context:
    foo: int = 0
    bar: str = 'qux'

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
# Make some modifications
ctx.foo += 1
ctx.bar = 'baz'
"#
            );
        });

        ctx.populate_from_extensions(&Extensions::new());

        Python::with_gil(|py| {
            py_run!(
                py,
                ctx,
                r#"
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
        Python::with_gil(|py| {
            py_run!(py, ctx, "assert ctx is None");
        });

        Ok(())
    }
}
