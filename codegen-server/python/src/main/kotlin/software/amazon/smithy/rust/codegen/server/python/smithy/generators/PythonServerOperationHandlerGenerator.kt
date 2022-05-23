/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerOperationHandlerGenerator
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.util.toSnakeCase

/**
 * The Rust code responsible to run the Python business logic on the Python interpreter
 * is implemented in this class, which inherits from [ServerOperationHandlerGenerator].
 *
 * We codegenerate all operations handlers (steps usually left to the developer in a pure
 * Rust application), which are built into a `Router` by [PythonApplicationGenerator].
 *
 * To call a Python function from Rust, anything dealing with Python runs inside an async
 * block that allows to catch stacktraces. The handler function is extracted from `PyHandler`
 * and called with the necessary arguments inside a blocking Tokio task.
 * At the end the block is awaited and errors are collected and reported.
 *
 * To call a Python coroutine, the same happens, but scheduled in a `tokio::Future`.
 */
class PythonServerOperationHandlerGenerator(
    codegenContext: CodegenContext,
    private val operations: List<OperationShape>,
) : ServerOperationHandlerGenerator(codegenContext, operations) {
    private val serverCrate = "aws_smithy_http_server"
    private val serverPythonCrate = "aws_smithy_http_server_python"
    private val symbolProvider = codegenContext.symbolProvider
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            "SmithyPython" to PythonServerCargoDependency.SmithyHttpServerPython(runtimeConfig).asType(),
            "SmithyServer" to ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType(),
            "pyo3" to PythonServerCargoDependency.PyO3.asType(),
            "pyo3asyncio" to PythonServerCargoDependency.PyO3Asyncio.asType(),
            "tokio" to PythonServerCargoDependency.Tokio.asType(),
            "tracing" to PythonServerCargoDependency.Tracing.asType()
        )

    override fun render(writer: RustWriter) {
        super.render(writer)
        renderPythonOperationHandlerImpl(writer)
    }

    private fun renderPythonOperationHandlerImpl(writer: RustWriter) {
        operations.map { operation ->
            val operationName = symbolProvider.toSymbol(operation).name
            val input = "crate::input::${operationName}Input"
            val output = "crate::output::${operationName}Output"
            val error = "crate::error::${operationName}Error"
            val name = operationName.toSnakeCase()

            writer.rustBlockTemplate(
                """
                /// Python handler for operation `$operationName`.
                pub async fn $name(
                    input: $input,
                    state: #{SmithyServer}::Extension<#{SmithyPython}::State>,
                    handler: std::sync::Arc<#{SmithyPython}::PyHandler>,
                ) -> Result<$output, $error>
                """.trimIndent(),
                *codegenScope
            ) {
                rustTemplate(
                    """
                    // Async block used to run the handler and catch any Python error.
                    let result = async {
                        let handler = handler.clone();
                        if handler.is_coroutine {
                            ${renderPyCoroutine(name, output)}
                        } else {
                            ${renderPyFunction(name, output)}
                        }
                    };
                    ${renderPyError()}
                    """.trimIndent(),
                    *codegenScope
                )
            }
        }
    }

    private fun renderPyFunction(name: String, output: String): String =
        """
        #{tracing}::debug!("Executing Python handlertion `$name()`");
        #{tokio}::task::spawn_blocking(move || {
            #{pyo3}::Python::with_gil(|py| {
                let pyhandler: &#{pyo3}::types::PyFunction = handler.extract(py)?;
                let output = if handler.args == 1 {
                    pyhandler.call1((input,))?
                } else {
                    pyhandler.call1((input, &*state.0.context))?
                };
                output.extract::<$output>()
            })
        })
        .await.map_err(|e| #{pyo3}::exceptions::PyRuntimeError::new_err(e.to_string()))?
        """

    private fun renderPyCoroutine(name: String, output: String): String =
        """
        #{tracing}::debug!("Executing Python coroutine `$name()`");
        let result = #{pyo3}::Python::with_gil(|py| {
            let pyhandler: &#{pyo3}::types::PyFunction = handler.extract(py)?;
            let coro = if handler.args == 1 {
                pyhandler.call1((input,))?
            } else {
                pyhandler.call1((input, &*state.0.context))?
            };
            #{pyo3asyncio}::tokio::into_future(coro)
        })?;
        result.await.map(|r| #{pyo3}::Python::with_gil(|py| r.extract::<$output>(py)))?
        """

    private fun renderPyError(): String =
        """
        // Catch and record and Python traceback.
        result.await.map_err(|e| {
            #{pyo3}::Python::with_gil(|py| {
                let traceback = match e.traceback(py) {
                    Some(t) => t.format().unwrap_or_else(|e| e.to_string()),
                    None => "Unknown traceback".to_string()
                };
                #{tracing}::error!("{}\n{}", e, traceback);
            });
            e.into()
        })
        """
}
