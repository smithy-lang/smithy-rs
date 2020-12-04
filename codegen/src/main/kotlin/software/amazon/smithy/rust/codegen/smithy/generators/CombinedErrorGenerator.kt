package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.OperationIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.lang.Derives
import software.amazon.smithy.rust.codegen.lang.RustMetadata
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.lang.rust
import software.amazon.smithy.rust.codegen.lang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeType

fun OperationShape.errorSymbol(symbolProvider: SymbolProvider): RuntimeType {
    val symbol = symbolProvider.toSymbol(this)
    return RuntimeType("${symbol.name}Error", null, "crate::error")
}

class CombinedErrorGenerator(
    model: Model,
    private val symbolProvider: SymbolProvider,
    private val operation: OperationShape
) {

    private val operationIndex = OperationIndex.of(model)
    fun render(writer: RustWriter) {
        val errors = operationIndex.getErrors(operation)
        val symbol = operation.errorSymbol(symbolProvider)
        val meta = RustMetadata(derives = Derives(setOf(RuntimeType.StdFmt("Debug"))), public = true)
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
                /// An unmodeled error, eg. a new error that was added since this SDK was generated
                Generic(#T)
            """,
                RuntimeType.StdError, RuntimeType.GenericError
            )
        }
        writer.rustBlock("impl #T for ${symbol.name}", RuntimeType.StdFmt("Display")) {
            rustBlock("fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result") {
                rustBlock("match self") {
                    errors.forEach {
                        val errorSymbol = symbolProvider.toSymbol(it)
                        rust("""${symbol.name}::${errorSymbol.name}(inner) => inner.fmt(f),""")
                    }
                    rust(
                        """
                    ${symbol.name}::Generic(inner) => inner.fmt(f),
                    ${symbol.name}::Unhandled(inner) => inner.fmt(f)
                    """
                    )
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
                    rust(
                        """
                    ${symbol.name}::Generic(inner) => Some(inner),
                    ${symbol.name}::Unhandled(inner) => Some(inner.as_ref())
                    """
                    )
                }
            }
        }
    }
}
