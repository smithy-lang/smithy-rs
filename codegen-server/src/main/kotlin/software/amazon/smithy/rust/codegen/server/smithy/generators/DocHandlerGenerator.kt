package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.Errors
import software.amazon.smithy.rust.codegen.core.smithy.Inputs
import software.amazon.smithy.rust.codegen.core.smithy.Outputs
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

/**
Generates a stub for use within documentation.
 */
class DocHandlerGenerator(private val operation: OperationShape, private val commentToken: String = "//", private val codegenContext: CodegenContext) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val crateName = codegenContext.settings.moduleName.toSnakeCase()

    /**
     * Returns the function signature for an operation handler implementation. Used in the documentation.
     */
    private fun OperationShape.docSignature(): Writable {
        val inputSymbol = symbolProvider.toSymbol(inputShape(model))
        val outputSymbol = symbolProvider.toSymbol(outputShape(model))
        val errorSymbol = errorSymbol(model, symbolProvider, CodegenTarget.SERVER)

        val outputT = if (errors.isEmpty()) {
            outputSymbol.name
        } else {
            "Result<${outputSymbol.name}, ${errorSymbol.name}>"
        }

        return writable {
            if (!errors.isEmpty()) {
                rust("$commentToken ## use $crateName::${Errors.namespace}::${errorSymbol.name};")
            }
            rust(
                """
                $commentToken ## use $crateName::${Inputs.namespace}::${inputSymbol.name};
                $commentToken ## use $crateName::${Outputs.namespace}::${outputSymbol.name};
                $commentToken async fn handler(input: ${inputSymbol.name}) -> $outputT {
                $commentToken     todo!()
                $commentToken }
                """.trimIndent(),
            )
        }
    }

    fun render(writer: RustWriter) {
        operation.docSignature()(writer)
    }
}
