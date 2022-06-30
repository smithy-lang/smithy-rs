/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ResourceShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.ServerCodegenContext

class PythonServerModuleGenerator(
    private val codegenContext: ServerCodegenContext,
    private val rustCrate: RustCrate,
    private val serviceShapes: Set<Shape>
) {
    private val codegenScope = arrayOf(
        "SmithyPython" to PythonServerCargoDependency.SmithyHttpServerPython(codegenContext.runtimeConfig).asType(),
        "pyo3" to PythonServerCargoDependency.PyO3.asType(),
    )
    private val symbolProvider = codegenContext.symbolProvider
    private val libName = "lib${codegenContext.settings.moduleName}"

    fun render() {
        rustCrate.withModule(
            RustModule.public("python_module_export", "Export PyO3 symbols in the shared library")
        ) { writer ->
            writer.rustBlockTemplate(
                """
                ##[#{pyo3}::pymodule]
                ##[#{pyo3}(name = "$libName")]
                pub fn python_library(py: #{pyo3}::Python<'_>, m: &#{pyo3}::types::PyModule) -> #{pyo3}::PyResult<()>
                """,
                *codegenScope
            ) {
                renderPyCodegeneratedTypes()
                renderPyWrapperTypes()
                renderPySocketType()
                renderPyApplicationType()
            }
        }
    }

    // Render codegenerated types that are wrapped with #[pyclass] attribute.
    private fun RustWriter.renderPyCodegeneratedTypes() {
        rustTemplate(
            """
            let input = #{pyo3}::types::PyModule::new(py, "input")?;
            let output = #{pyo3}::types::PyModule::new(py, "output")?;
            let error = #{pyo3}::types::PyModule::new(py, "error")?;
            let model = #{pyo3}::types::PyModule::new(py, "model")?;
            """,
            *codegenScope
        )
        serviceShapes.forEach() { shape ->
            val moduleType = moduleType(shape)
            if (moduleType != null) {
                rustTemplate(
                    """
                    $moduleType.add_class::<crate::$moduleType::${shape.id.name}>()?;
                    """,
                    *codegenScope
                )
            }
        }
        rustTemplate(
            """
            #{pyo3}::py_run!(py, input, "import sys; sys.modules['$libName.input'] = input");
            m.add_submodule(input)?;
            #{pyo3}::py_run!(py, output, "import sys; sys.modules['$libName.output'] = output");
            m.add_submodule(output)?;
            #{pyo3}::py_run!(py, error, "import sys; sys.modules['$libName.error'] = error");
            m.add_submodule(error)?;
            #{pyo3}::py_run!(py, model, "import sys; sys.modules['$libName.model'] = model");
            m.add_submodule(model)?;
            """,
            *codegenScope
        )
    }

    // Render wrapper types that are substituted to the ones coming from `aws_smithy_types`.
    private fun RustWriter.renderPyWrapperTypes() {
        rustTemplate(
            """
            let types = #{pyo3}::types::PyModule::new(py, "types")?;
            types.add_class::<#{SmithyPython}::types::Blob>()?;
            types.add_class::<#{SmithyPython}::types::DateTime>()?;
            #{pyo3}::py_run!(
                py,
                types,
                "import sys; sys.modules['$libName.types'] = types"
            );
            m.add_submodule(types)?;
            """,
            *codegenScope
        )
    }

    // Render Python shared socket type.
    private fun RustWriter.renderPySocketType() {

        rustTemplate(
            """
            let socket = #{pyo3}::types::PyModule::new(py, "socket")?;
            socket.add_class::<#{SmithyPython}::PySocket>()?;
            #{pyo3}::py_run!(
                py,
                socket,
                "import sys; sys.modules['$libName.types'] = socket"
            );
            m.add_submodule(socket)?;
            """,
            *codegenScope
        )
    }

    // Render Python application type.
    private fun RustWriter.renderPyApplicationType() {
        rustTemplate(
            """
            m.add_class::<crate::python_server_application::App>()?;
            Ok(())
            """,
            *codegenScope
        )
    }

    // Convert to symbol and check the namespace to figure out where they should be imported from.
    private fun moduleType(shape: Shape): String? {
        when (shape) {
            // Shapes that should never be exposed to Python directly
            is ServiceShape, is ResourceShape, is OperationShape, is MemberShape -> {}
            else -> {
                val namespace = symbolProvider.toSymbol(shape).namespace
                if (!namespace.isEmpty() && namespace.startsWith("crate")) {
                    return namespace.split("::").last()
                }
            }
        }
        return null
    }
}
