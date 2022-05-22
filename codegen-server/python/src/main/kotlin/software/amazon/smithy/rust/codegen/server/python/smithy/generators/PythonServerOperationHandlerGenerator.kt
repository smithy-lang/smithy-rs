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
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.util.toSnakeCase

/**
 * ServerOperationHandlerGenerator
 */
class PythonServerOperationHandlerGenerator(
    codegenContext: CodegenContext,
    private val operations: List<OperationShape>,
) {
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

    fun render(writer: RustWriter) {
        renderOperationHandlerImpl(writer)
    }

    private fun renderOperationHandlerImpl(writer: RustWriter) {
        operations.map { operation ->
            val operationName = symbolProvider.toSymbol(operation).name
            val inputName = "crate::input::${operationName}Input"
            val outputName = "crate::output::${operationName}Output"
            val errorName = "crate::error::${operationName}Error"
            val name = operationName.toSnakeCase()

            writer.rustBlockTemplate(
                """
                /// Python handler for operation `$operationName`.
                pub async fn $name(input: $inputName, state: #{SmithyServer}::Extension<#{SmithyPython}::State>) -> Result<$outputName, $errorName>
                """.trimIndent(),
                *codegenScope
            ) {
                rustTemplate(
                    """
                    // Async block used to run the handler and catch any Python error.
                    let result = async {
                        let func = state.0.handlers.get("$name").ok_or(#{pyo3}::exceptions::PyRuntimeError::new_err(
                            "Unable to find Python handler for operation `$operationName`".to_string()
                        ))?;
                        let func = func.clone();
                        if func.is_coroutine {
                            #{tracing}::debug!("Executing Python coroutine `$name()`");
                            let result = #{pyo3}::Python::with_gil(|py| {
                                let pyfunc: &#{pyo3}::types::PyFunction = func.extract(py)?;
                                let coro = if func.args == 1 {
                                    pyfunc.call1((input,))?
                                } else {
                                    pyfunc.call1((input, &*state.0.context))?
                                };
                                #{pyo3asyncio}::tokio::into_future(coro)
                            })?;
                            result.await.map(|r| #{pyo3}::Python::with_gil(|py| r.extract::<$outputName>(py)))?
                        } else {
                            #{tracing}::debug!("Executing Python function `$name()`");
                            #{tokio}::task::spawn_blocking(move || {
                                #{pyo3}::Python::with_gil(|py| {
                                    let pyfunc: &#{pyo3}::types::PyFunction = func.extract(py)?;
                                    let output = if func.args == 1 {
                                        pyfunc.call1((input,))?
                                    } else {
                                        pyfunc.call1((input, &*state.0.context))?
                                    };
                                    output.extract::<$outputName>()
                                })
                            })
                            .await.map_err(|e| #{pyo3}::exceptions::PyRuntimeError::new_err(e.to_string()))?
                        }
                    };
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
                    """.trimIndent(),
                    *codegenScope
                )
            }
        }
    }
}
