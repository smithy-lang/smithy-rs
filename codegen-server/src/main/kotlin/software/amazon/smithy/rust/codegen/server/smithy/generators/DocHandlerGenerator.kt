/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.ErrorsModule
import software.amazon.smithy.rust.codegen.core.smithy.InputsModule
import software.amazon.smithy.rust.codegen.core.smithy.OutputsModule
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.outputShape

/**
 * Generates a handler implementation stub for use within documentation.
 */
class DocHandlerGenerator(
    codegenContext: CodegenContext,
    private val operation: OperationShape,
    private val handlerName: String,
    private val commentToken: String = "//",
) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider

    private val inputSymbol = symbolProvider.toSymbol(operation.inputShape(model))
    private val outputSymbol = symbolProvider.toSymbol(operation.outputShape(model))
    private val errorSymbol = operation.errorSymbol(symbolProvider)

    /**
     * Returns the function signature for an operation handler implementation. Used in the documentation.
     */
    fun docSignature(): Writable {
        val outputT = if (operation.errors.isEmpty()) {
            "${OutputsModule.name}::${outputSymbol.name}"
        } else {
            "Result<${OutputsModule.name}::${outputSymbol.name}, ${ErrorsModule.name}::${errorSymbol.name}>"
        }

        return writable {
            rust(
                """
                $commentToken async fn $handlerName(input: ${InputsModule.name}::${inputSymbol.name}) -> $outputT {
                $commentToken     todo!()
                $commentToken }
                """.trimIndent(),
            )
        }
    }

    fun render(writer: RustWriter) {
        // This assumes that the `error` (if applicable) `input`, and `output` modules have been imported by the
        // caller and hence are in scope.
        writer.rustTemplate(
            """
            #{Handler:W}
            """,
            "Handler" to docSignature(),
        )
    }
}
