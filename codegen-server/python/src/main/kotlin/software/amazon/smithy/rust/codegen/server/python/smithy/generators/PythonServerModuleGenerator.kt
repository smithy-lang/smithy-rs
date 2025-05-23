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
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.Version
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerRustModule
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext

class PythonServerModuleGenerator(
    codegenContext: ServerCodegenContext,
    private val rustCrate: RustCrate,
    private val serviceShapes: Set<Shape>,
) {
    private val codegenScope =
        arrayOf(
            "SmithyPython" to PythonServerCargoDependency.smithyHttpServerPython(codegenContext.runtimeConfig).toType(),
            "pyo3" to PythonServerCargoDependency.PyO3.toType(),
        )
    private val symbolProvider = codegenContext.symbolProvider
    private val libName = codegenContext.settings.moduleName.toSnakeCase()

    fun render() {
        rustCrate.withModule(PythonServerRustModule.PythonModuleExport) {
            rustBlockTemplate(
                """
                ##[#{pyo3}::pymodule]
                ##[pyo3(name = "$libName")]
                pub fn python_library(py: #{pyo3}::Python<'_>, m: &#{pyo3}::types::PyModule) -> #{pyo3}::PyResult<()>
                """,
                *codegenScope,
            ) {
                renderPyCodegeneratedTypes()
                renderPyWrapperTypes()
                renderPySocketType()
                renderPyLogging()
                renderPyMiddlewareTypes()
                renderPyTlsTypes()
                renderPyLambdaTypes()
                renderPyApplicationType()
                renderCodegenVersion()
                rust("Ok(())")
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
            """,
            *codegenScope,
        )
        // The `model` type section can be unused in models like `simple`, so we accommodate for it.
        var visitedModelType = false
        serviceShapes.forEach { shape ->
            val moduleType = moduleType(shape)
            if (moduleType != null) {
                if (moduleType == "model" && !visitedModelType) {
                    rustTemplate(
                        """
                        let model = #{pyo3}::types::PyModule::new(py, "model")?;
                        """,
                        *codegenScope,
                    )
                    visitedModelType = true
                }
                when (shape) {
                    is UnionShape ->
                        rustTemplate(
                            """
                            $moduleType.add_class::<crate::$moduleType::PyUnionMarker${shape.id.name.toPascalCase()}>()?;
                            """,
                            *codegenScope,
                        )
                    else ->
                        rustTemplate(
                            """
                            $moduleType.add_class::<crate::$moduleType::${shape.id.name.toPascalCase()}>()?;
                            """,
                            *codegenScope,
                        )
                }
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
            """,
            *codegenScope,
        )
        if (visitedModelType) {
            rustTemplate(
                """
                #{pyo3}::py_run!(py, model, "import sys; sys.modules['$libName.model'] = model");
                m.add_submodule(model)?;
                """,
                *codegenScope,
            )
        }
    }

    // Render wrapper types that are substituted to the ones coming from `aws_smithy_types`.
    private fun RustWriter.renderPyWrapperTypes() {
        rustTemplate(
            """
            let types = #{pyo3}::types::PyModule::new(py, "types")?;
            types.add_class::<#{SmithyPython}::types::Blob>()?;
            types.add_class::<#{SmithyPython}::types::DateTime>()?;
            types.add_class::<#{SmithyPython}::types::Format>()?;
            types.add_class::<#{SmithyPython}::types::ByteStream>()?;
            #{pyo3}::py_run!(
                py,
                types,
                "import sys; sys.modules['$libName.types'] = types"
            );
            m.add_submodule(types)?;
            """,
            *codegenScope,
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
                "import sys; sys.modules['$libName.socket'] = socket"
            );
            m.add_submodule(socket)?;
            """,
            *codegenScope,
        )
    }

    // Render Python shared socket type.
    private fun RustWriter.renderPyLogging() {
        rustTemplate(
            """
            let logging = #{pyo3}::types::PyModule::new(py, "logging")?;
            logging.add_function(#{pyo3}::wrap_pyfunction!(#{SmithyPython}::py_tracing_event, m)?)?;
            logging.add_class::<#{SmithyPython}::PyTracingHandler>()?;
            #{pyo3}::py_run!(
                py,
                logging,
                "import sys; sys.modules['$libName.logging'] = logging"
            );
            m.add_submodule(logging)?;
            """,
            *codegenScope,
        )
    }

    private fun RustWriter.renderPyMiddlewareTypes() {
        rustTemplate(
            """
            let middleware = #{pyo3}::types::PyModule::new(py, "middleware")?;
            middleware.add_class::<#{SmithyPython}::PyRequest>()?;
            middleware.add_class::<#{SmithyPython}::PyResponse>()?;
            middleware.add_class::<#{SmithyPython}::PyMiddlewareException>()?;
            pyo3::py_run!(
                py,
                middleware,
                "import sys; sys.modules['$libName.middleware'] = middleware"
            );
            m.add_submodule(middleware)?;
            """,
            *codegenScope,
        )
    }

    private fun RustWriter.renderPyTlsTypes() {
        rustTemplate(
            """
            let tls = #{pyo3}::types::PyModule::new(py, "tls")?;
            tls.add_class::<#{SmithyPython}::tls::PyTlsConfig>()?;
            pyo3::py_run!(
                py,
                tls,
                "import sys; sys.modules['$libName.tls'] = tls"
            );
            m.add_submodule(tls)?;
            """,
            *codegenScope,
        )
    }

    private fun RustWriter.renderPyLambdaTypes() {
        rustTemplate(
            """
            let aws_lambda = #{pyo3}::types::PyModule::new(py, "aws_lambda")?;
            aws_lambda.add_class::<#{SmithyPython}::lambda::PyLambdaContext>()?;
            aws_lambda.add_class::<#{SmithyPython}::lambda::PyClientApplication>()?;
            aws_lambda.add_class::<#{SmithyPython}::lambda::PyClientContext>()?;
            aws_lambda.add_class::<#{SmithyPython}::lambda::PyCognitoIdentity>()?;
            aws_lambda.add_class::<#{SmithyPython}::lambda::PyConfig>()?;
            pyo3::py_run!(
                py,
                aws_lambda,
                "import sys; sys.modules['$libName.aws_lambda'] = aws_lambda"
            );
            m.add_submodule(aws_lambda)?;
            """,
            *codegenScope,
        )
    }

    // Render Python application type.
    private fun RustWriter.renderPyApplicationType() {
        rust("""m.add_class::<crate::python_server_application::App>()?;""")
    }

    // Render the codegeneration version as module attribute.
    private fun RustWriter.renderCodegenVersion() {
        rust("""m.add("CODEGEN_VERSION", "${Version.fromDefaultResource().gitHash}")?;""")
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
