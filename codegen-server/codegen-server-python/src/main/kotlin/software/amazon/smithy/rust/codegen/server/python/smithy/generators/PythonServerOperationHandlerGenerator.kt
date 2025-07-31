/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency

/**
 * The Rust code responsible to run the Python business logic on the Python interpreter
 * is implemented in this class, which inherits from [ServerOperationHandlerGenerator].
 *
 * We codegenerate all operations handlers (steps usually left to the developer in a pure
 * Rust application), which are built into a `Router` by [PythonApplicationGenerator].
 *
 * To call a Python function from Rust, anything dealing with Python runs inside an async
 * block that allows to catch stack traces. The handler function is extracted from `PyHandler`
 * and called with the necessary arguments inside a blocking Tokio task.
 * At the end the block is awaited and errors are collected and reported.
 *
 * To call a Python coroutine, the same happens, but scheduled in a `tokio::Future`.
 */
class PythonServerOperationHandlerGenerator(
    codegenContext: CodegenContext,
    private val operation: OperationShape,
) {
    private val symbolProvider = codegenContext.symbolProvider
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            "SmithyPython" to PythonServerCargoDependency.smithyHttpServerPython(runtimeConfig).toType(),
            "SmithyServer" to PythonServerCargoDependency.smithyHttpServer(runtimeConfig).toType(),
            "pyo3" to PythonServerCargoDependency.PyO3.toType(),
            "pyo3_asyncio" to PythonServerCargoDependency.PyO3Asyncio.toType(),
            "tokio" to PythonServerCargoDependency.Tokio.toType(),
            "tracing" to PythonServerCargoDependency.Tracing.toType(),
        )

    fun render(writer: RustWriter) {
        renderPythonOperationHandlerImpl(writer)
    }

    private fun renderPythonOperationHandlerImpl(writer: RustWriter) {
        val operationName = symbolProvider.toSymbol(operation).name
        val input = "crate::input::${operationName.toPascalCase()}Input"
        val output = "crate::output::${operationName.toPascalCase()}Output"
        // TODO(https://github.com/smithy-lang/smithy-rs/issues/2552) - Use to pascalCase for error shapes.
        val error = "crate::error::${operationName}Error"
        val fnName = RustReservedWords.escapeIfNeeded(symbolProvider.toSymbol(operation).name.toSnakeCase())

        writer.rustTemplate(
            """
            /// Python handler for operation `$operationName`.
            pub(crate) async fn $fnName(
                input: $input,
                state: #{SmithyServer}::Extension<#{SmithyPython}::context::PyContext>,
                handler: #{SmithyPython}::PyHandler,
            ) -> std::result::Result<$output, $error> {
                // Async block used to run the handler and catch any Python error.
                let result = if handler.is_coroutine {
                    #{PyCoroutine:W}
                } else {
                    #{PyFunction:W}
                };
                #{PyError:W}
            }
            """,
            *codegenScope,
            "PyCoroutine" to renderPyCoroutine(fnName, output),
            "PyFunction" to renderPyFunction(fnName, output),
            "PyError" to renderPyError(),
        )
    }

    private fun renderPyFunction(
        name: String,
        output: String,
    ): Writable =
        writable {
            rustTemplate(
                """
                #{tracing}::trace!(name = "$name", "executing python handler function");
                #{pyo3}::Python::with_gil(|py| {
                    let pyhandler: &#{pyo3}::types::PyFunction = handler.extract(py)?;
                    let output = if handler.args == 1 {
                        pyhandler.call1((input,))?
                    } else {
                        pyhandler.call1((input, #{pyo3}::ToPyObject::to_object(&state.0, py)))?
                    };
                    output.extract::<$output>()
                })
                """,
                *codegenScope,
            )
        }

    private fun renderPyCoroutine(
        name: String,
        output: String,
    ): Writable =
        writable {
            rustTemplate(
                """
                #{tracing}::trace!(name = "$name", "executing python handler coroutine");
                let result = #{pyo3}::Python::with_gil(|py| {
                    let pyhandler: &#{pyo3}::types::PyFunction = handler.extract(py)?;
                    let coroutine = if handler.args == 1 {
                        pyhandler.call1((input,))?
                    } else {
                        pyhandler.call1((input, #{pyo3}::ToPyObject::to_object(&state.0, py)))?
                    };
                    #{pyo3_asyncio}::tokio::into_future(coroutine)
                })?;
                result.await.and_then(|r| #{pyo3}::Python::with_gil(|py| r.extract::<$output>(py)))
                """,
                *codegenScope,
            )
        }

    private fun renderPyError(): Writable =
        writable {
            rustTemplate(
                """
                // Catch and record a Python traceback.
                result.map_err(|e| {
                    let rich_py_err = #{SmithyPython}::rich_py_err(#{pyo3}::Python::with_gil(|py| { e.clone_ref(py) }));
                    #{tracing}::error!(error = ?rich_py_err, "handler error");
                    e.into()
                })
                """,
                *codegenScope,
            )
        }
}
