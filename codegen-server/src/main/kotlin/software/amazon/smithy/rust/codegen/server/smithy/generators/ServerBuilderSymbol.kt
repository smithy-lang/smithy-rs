package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.smithy.serverBuilderSymbol
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext

fun StructureShape.serverBuilderSymbol(codegenContext: ServerCodegenContext): Symbol =
    this.serverBuilderSymbol(codegenContext.symbolProvider, !codegenContext.settings.codegenConfig.publicConstrainedTypes)
