/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule.Error as ErrorModule
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule.Input as InputModule
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule.Output as OutputModule

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
    private val errorSymbol = symbolProvider.symbolForOperationError(operation)

    /**
     * Returns the function signature for an operation handler implementation. Used in the documentation.
     */
    fun docSignature(): Writable {
        val outputT = if (operation.errors.isEmpty()) {
            "${OutputModule.name}::${outputSymbol.name}"
        } else {
            "Result<${OutputModule.name}::${outputSymbol.name}, ${ErrorModule.name}::${errorSymbol.name}>"
        }

        return writable {
            rust(
                """
                $commentToken async fn $handlerName(input: ${InputModule.name}::${inputSymbol.name}) -> $outputT {
                $commentToken     todo!()
                $commentToken }
                """.trimIndent(),
            )
        }
    }

    /**
     * Similarly to `docSignature`, returns the function signature of an operation handler implementation, with the
     * difference that we don't ellide the error for use in `tower::service_fn`.
     */
    fun docFixedSignature(): Writable {
        val errorT = if (operation.errors.isEmpty()) {
            "std::convert::Infallible"
        } else {
            "${ErrorModule.name}::${errorSymbol.name}"
        }

        return writable {
            rust(
                """
                $commentToken async fn $handlerName(input: ${InputModule.name}::${inputSymbol.name}) -> Result<${OutputModule.name}::${outputSymbol.name}, $errorT> {
                $commentToken     todo!()
                $commentToken }
                """.trimIndent(),
            )
        }
    }
}
