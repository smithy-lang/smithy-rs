/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerOperationHandlerGenerator
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
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
    coreCodegenContext: CoreCodegenContext,
    private val operations: List<OperationShape>,
) : ServerOperationHandlerGenerator(coreCodegenContext, operations) {
    private val symbolProvider = coreCodegenContext.symbolProvider
    private val runtimeConfig = coreCodegenContext.runtimeConfig
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
        for (operation in operations) {
            val operationName = symbolProvider.toSymbol(operation).name
            val input = "crate::input::${operationName}Input"
            val output = "crate::output::${operationName}Output"
            val error = "crate::error::${operationName}Error"
            val fnName = operationName.toSnakeCase()

            writer.rustTemplate(
                """
                /// Python handler for operation `$operationName`.
                pub async fn $fnName(
                    input: $input,
                    state: #{SmithyServer}::Extension<#{SmithyPython}::PyState>,
                    handler: std::sync::Arc<#{SmithyPython}::PyHandler>,
                ) -> std::result::Result<$output, $error> {
                    // Async block used to run the handler and catch any Python error.
                    let result = async {
                        let handler = handler.clone();
                        if handler.is_coroutine {
                            #{pycoroutine:W}
                        } else {
                            #{pyfunction:W}
                        }
                    };
                    #{pyerror:W}
                }
                """,
                *codegenScope,
                "pycoroutine" to renderPyCoroutine(fnName, output),
                "pyfunction" to renderPyFunction(fnName, output),
                "pyerror" to renderPyError(),
            )
        }
    }

    private fun renderPyFunction(name: String, output: String): Writable =
        writable {
            rustTemplate(
                """
                #{tracing}::debug!("Executing Python handler function `$name()`");
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
                """,
                *codegenScope
            )
        }

    private fun renderPyCoroutine(name: String, output: String): Writable =
        writable {
            rustTemplate(
                """
                #{tracing}::debug!("Executing Python handler coroutine `$name()`");
                let result = #{pyo3}::Python::with_gil(|py| {
                    let pyhandler: &#{pyo3}::types::PyFunction = handler.extract(py)?;
                    let coroutine = if handler.args == 1 {
                        pyhandler.call1((input,))?
                    } else {
                        pyhandler.call1((input, &*state.0.context))?
                    };
                    #{pyo3asyncio}::tokio::into_future(coroutine)
                })?;
                result.await.map(|r| #{pyo3}::Python::with_gil(|py| r.extract::<$output>(py)))?
                """,
                *codegenScope
            )
        }

    private fun renderPyError(): Writable =
        writable {
            rustTemplate(
                """
                // Catch and record a Python traceback.
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
                """,
                *codegenScope
            )
        }
}
