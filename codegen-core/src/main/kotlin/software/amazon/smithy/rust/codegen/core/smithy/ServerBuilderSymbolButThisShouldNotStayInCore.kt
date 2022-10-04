package software.amazon.smithy.rust.codegen.core.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

// TODO Move this to `ServerBuilderSymbol` in codegen-server
fun StructureShape.serverBuilderSymbol(symbolProvider: SymbolProvider, pubCrate: Boolean): Symbol {
    val structureSymbol = symbolProvider.toSymbol(this)
    val builderNamespace = RustReservedWords.escapeIfNeeded(structureSymbol.name.toSnakeCase()) +
        if (pubCrate) {
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

