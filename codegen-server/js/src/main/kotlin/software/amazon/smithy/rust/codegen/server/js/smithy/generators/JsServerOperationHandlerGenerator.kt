/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.js.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.js.smithy.JsServerCargoDependency

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
class JsServerOperationHandlerGenerator(
    codegenContext: CodegenContext,
    private val operation: OperationShape,
) {
    private val symbolProvider = codegenContext.symbolProvider
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            "SmithyJs" to JsServerCargoDependency.smithyHttpServerJs(runtimeConfig).toType(),
            "SmithyServer" to JsServerCargoDependency.smithyHttpServer(runtimeConfig).toType(),
            "napi" to JsServerCargoDependency.Napi.toType(),
        )

    fun render(writer: RustWriter) {
        renderJsOperationHandlerImpl(writer)
    }

    private fun renderJsOperationHandlerImpl(writer: RustWriter) {
        val operationName = symbolProvider.toSymbol(operation).name
        val input = "crate::input::${operationName}Input"
        val output = "crate::output::${operationName}Output"
        val error = "crate::error::${operationName}Error"
        val fnName = operationName.toSnakeCase()

        writer.rustTemplate(
            """
            /// Python handler for operation `$operationName`.
            pub(crate) async fn $fnName(
                input: $input,
                handlers: #{SmithyServer}::Extension<crate::js_server_application::Handlers>,
            ) -> std::result::Result<$output, $error> {
                handlers.$fnName.call_async::<#{napi}::bindgen_prelude::Promise<$output>>(Ok(input)).await?.await.map_err(|e| e.into())
            }
            """,
            *codegenScope,
        )
    }
}
