package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.OperationIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.lang.Attribute
import software.amazon.smithy.rust.codegen.lang.Derives
import software.amazon.smithy.rust.codegen.lang.RustMetadata
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.lang.rust
import software.amazon.smithy.rust.codegen.lang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeType

/**
 * For a given Operation ([this]), return the symbol referring to the unified error? This can be used
 * if you, eg. want to return a unfied error from a function:
 *
 * ```kotlin
 * rustWriter.rustBlock("fn get_error() -> #T", operation.errorSymbol(symbolProvider)) {
 *   write("todo!() // function body")
 * }
 * ```
 */
fun OperationShape.errorSymbol(symbolProvider: SymbolProvider): RuntimeType {
    val symbol = symbolProvider.toSymbol(this)
    return RuntimeType("${symbol.name}Error", null, "crate::error")
}

/**
 * Generates a unified error enum for [operation]. [ErrorGenerator] handles generating the individual variants,
 * but we must still combine those variants into an enum covering all possible errors for a given operation.
 */
class CombinedErrorGenerator(
    model: Model,
    private val symbolProvider: SymbolProvider,
    private val operation: OperationShape
) {

    private val operationIndex = OperationIndex.of(model)
    fun render(writer: RustWriter) {
        val errors = operationIndex.getErrors(operation)
        val symbol = operation.errorSymbol(symbolProvider)
        val meta = RustMetadata(
            derives = Derives(setOf(RuntimeType.StdFmt("Debug"))),
            additionalAttributes = listOf(Attribute.NonExhaustive),
            public = true
        )
        meta.render(writer)
        writer.rustBlock("enum ${symbol.name}") {
            errors.forEach { errorVariant ->
                val errorSymbol = symbolProvider.toSymbol(errorVariant)
                write("${errorSymbol.name}(#T),", errorSymbol)
            }
            rust(
                """
                /// An unexpected error, eg. invalid JSON returned by the service
                Unhandled(Box<dyn #T>),
            """,
                RuntimeType.StdError
            )
        }
        writer.rustBlock("impl #T for ${symbol.name}", RuntimeType.StdFmt("Display")) {
            rustBlock("fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result") {
                rustBlock("match self") {
                    errors.forEach {
                        val errorSymbol = symbolProvider.toSymbol(it)
                        rust("""${symbol.name}::${errorSymbol.name}(inner) => inner.fmt(f),""")
                    }
                    rust("${symbol.name}::Unhandled(inner) => inner.fmt(f)")
                }
            }
        }

        writer.rustBlock("impl #T for ${symbol.name}", RuntimeType.StdError) {
            rustBlock("fn source(&self) -> Option<&(dyn #T + 'static)>", RuntimeType.StdError) {
                rustBlock("match self") {
                    errors.forEach {
                        val errorSymbol = symbolProvider.toSymbol(it)
                        rust("""${symbol.name}::${errorSymbol.name}(inner) => Some(inner),""")
                    }
                    rust("${symbol.name}::Unhandled(inner) => Some(inner.as_ref())")
                }
            }
        }
    }
}
