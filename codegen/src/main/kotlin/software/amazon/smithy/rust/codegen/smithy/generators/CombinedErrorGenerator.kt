package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.model.knowledge.OperationIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.lang.RustMetadata
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.lang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.BaseSymbolMetadataProvider
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider

fun OperationShape.errorSymbol(symbolProvider: RustSymbolProvider): RuntimeType {
    val symbol = symbolProvider.toSymbol(this)
    return RuntimeType("${symbol.name}Error", null, "crate::error")
}

class CombinedErrorGenerator(private val protocolConfig: ProtocolConfig, private val operation: OperationShape) {

    private val operationIndex = OperationIndex.of(protocolConfig.model)
    private val symbolProvider = protocolConfig.symbolProvider
    fun render(writer: RustWriter) {
        val errors = operationIndex.getErrors(operation)
        val symbol = operation.errorSymbol(symbolProvider)
        val meta = RustMetadata(derives = BaseSymbolMetadataProvider.defaultDerives, public = true)
        meta.render(writer)
        writer.rustBlock("enum ${symbol.name}") {
            errors.forEach { errorVariant ->
                val errorSymbol = symbolProvider.toSymbol(errorVariant)
                write("${errorSymbol.name}(\$T),", errorSymbol)
            }
            write("Unknown(String)")
        }
    }
}
