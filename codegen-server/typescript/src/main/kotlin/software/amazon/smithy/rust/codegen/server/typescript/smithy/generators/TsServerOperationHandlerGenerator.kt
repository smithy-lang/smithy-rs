/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.typescript.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.typescript.smithy.TsServerCargoDependency

/**
 * The Rust code responsible to run the Typescript business logic is implemented in this class,.
 *
 * We codegenerate all operations handlers (steps usually left to the developer in a pure
 * Rust application), which are built into a `Router` by [TsServerApplicationGenerator].
 */
class TsServerOperationHandlerGenerator(
    codegenContext: CodegenContext,
    private val operation: OperationShape,
) {
    private val symbolProvider = codegenContext.symbolProvider
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            "SmithyTs" to TsServerCargoDependency.smithyHttpServerTs(runtimeConfig).toType(),
            "SmithyServer" to TsServerCargoDependency.smithyHttpServer(runtimeConfig).toType(),
            "napi" to TsServerCargoDependency.Napi.toType(),
        )

    fun render(writer: RustWriter) {
        renderTsOperationHandlerImpl(writer)
    }

    private fun renderTsOperationHandlerImpl(writer: RustWriter) {
        val operationName = symbolProvider.toSymbol(operation).name
        val input = "crate::input::${operationName}Input"
        val output = "crate::output::${operationName}Output"
        val error = "crate::error::${operationName}Error"
        val fnName = operationName.toSnakeCase()

        writer.rustTemplate(
            """
            /// Typescript handler for operation `$operationName`.
            pub(crate) async fn $fnName(
                input: $input,
                handlers: #{SmithyServer}::Extension<crate::ts_server_application::Handlers>,
            ) -> std::result::Result<$output, $error> {
                handlers.$fnName.call_async::<#{napi}::bindgen_prelude::Promise<$output>>(input).await?.await.map_err(|e| e.into())
            }
            """,
            *codegenScope,
        )
    }
}
