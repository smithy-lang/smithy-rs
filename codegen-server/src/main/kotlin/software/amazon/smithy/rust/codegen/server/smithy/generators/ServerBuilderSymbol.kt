package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext

fun StructureShape.serverBuilderSymbol(codegenContext: ServerCodegenContext): Symbol {
    val structureSymbol = codegenContext.symbolProvider.toSymbol(this)
    val builderNamespace = RustReservedWords.escapeIfNeeded(structureSymbol.name.toSnakeCase()) +
        if (!codegenContext.settings.codegenConfig.publicConstrainedTypes) {
            "_internal"
        } else {
            ""
        }
    val rustType = RustType.Opaque("Builder", "${structureSymbol.namespace}::$builderNamespace")
    return Symbol.builder()
        .rustType(rustType)
        .name(rustType.name)
        .namespace(rustType.namespace, "::")
        .definitionFile(structureSymbol.definitionFile)
        .build()
}
