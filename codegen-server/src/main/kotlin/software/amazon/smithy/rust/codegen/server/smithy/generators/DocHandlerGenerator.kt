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
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.ErrorsModule
import software.amazon.smithy.rust.codegen.core.smithy.InputsModule
import software.amazon.smithy.rust.codegen.core.smithy.OutputsModule
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.outputShape

/**
Generates a stub for use within documentation.
 */
class DocHandlerGenerator(
    codegenContext: CodegenContext,
    private val operation: OperationShape,
    private val handlerName: String,
    private val commentToken: String = "//",
) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val crateName = codegenContext.moduleUseName()

    private val inputSymbol = symbolProvider.toSymbol(operation.inputShape(model))
    private val outputSymbol = symbolProvider.toSymbol(operation.outputShape(model))
    private val errorSymbol = operation.errorSymbol(model, symbolProvider, CodegenTarget.SERVER)

    /**
     * Returns the imports required for the function signature
     */
    fun docSignatureImports(): Writable = writable {
        if (operation.errors.isNotEmpty()) {
            rust("$commentToken use $crateName::${ErrorsModule.name}::${errorSymbol.name};")
        }
        rust(
            """
            $commentToken use $crateName::${InputsModule.name}::${inputSymbol.name};
            $commentToken use $crateName::${OutputsModule.name}::${outputSymbol.name};
            """.trimIndent(),
        )
    }

    /**
     * Returns the function signature for an operation handler implementation. Used in the documentation.
     */
    fun docSignature(): Writable {
        val outputT = if (operation.errors.isEmpty()) {
            outputSymbol.name
        } else {
            "Result<${outputSymbol.name}, ${errorSymbol.name}>"
        }

        return writable {
            rust(
                """
                $commentToken async fn $handlerName(input: ${inputSymbol.name}) -> $outputT {
                $commentToken     todo!()
                $commentToken }
                """.trimIndent(),
            )
        }
    }

    fun render(writer: RustWriter) {
        writer.rustTemplate(
            """
            #{Docs:W}
            $commentToken
            #{Handler:W}
            """,
            "Docs" to docSignatureImports(),
            "Handler" to docSignature(),
        )
    }
}
