/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency

internal fun RustWriter.renderPyBoxTraits(shapeName: String) {
    rustTemplate(
        """
        impl<'source> #{pyo3}::FromPyObject<'source> for std::boxed::Box<$shapeName> {
            fn extract(ob: &'source #{pyo3}::PyAny) -> #{pyo3}::PyResult<Self> {
                ob.extract::<$shapeName>().map(Box::new)
            }
        }

        impl #{pyo3}::IntoPy<#{pyo3}::PyObject> for std::boxed::Box<$shapeName> {
            fn into_py(self, py: #{pyo3}::Python<'_>) -> #{pyo3}::PyObject {
                (*self).into_py(py)
            }
        }
        """,
        "pyo3" to PythonServerCargoDependency.PyO3.toType(),
    )
}
