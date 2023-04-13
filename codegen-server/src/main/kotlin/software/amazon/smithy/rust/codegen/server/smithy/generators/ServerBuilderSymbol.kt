/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.smithy.module
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext

// TODO(https://github.com/awslabs/smithy-rs/issues/2396): Replace this with `RustSymbolProvider.symbolForBuilder`
fun StructureShape.serverBuilderSymbol(codegenContext: ServerCodegenContext): Symbol =
    this.serverBuilderSymbol(
        codegenContext.symbolProvider,
        !codegenContext.settings.codegenConfig.publicConstrainedTypes,
    )

// TODO(https://github.com/awslabs/smithy-rs/issues/2396): Replace this with `RustSymbolProvider.moduleForBuilder`
fun StructureShape.serverBuilderModule(symbolProvider: SymbolProvider, pubCrate: Boolean): RustModule.LeafModule {
    val structureSymbol = symbolProvider.toSymbol(this)
    val builderNamespace = RustReservedWords.escapeIfNeeded(structureSymbol.name.toSnakeCase()) +
        if (pubCrate) {
            "_internal"
        } else {
            ""
        }
    val visibility = when (pubCrate) {
        true -> Visibility.PUBCRATE
        false -> Visibility.PUBLIC
    }
    return RustModule.new(
        builderNamespace,
        visibility,
        parent = structureSymbol.module(),
        inline = true,
        documentationOverride = "",
    )
}

// TODO(https://github.com/awslabs/smithy-rs/issues/2396): Replace this with `RustSymbolProvider.symbolForBuilder`
fun StructureShape.serverBuilderSymbol(symbolProvider: SymbolProvider, pubCrate: Boolean): Symbol {
    val builderModule = serverBuilderModule(symbolProvider, pubCrate)
    val rustType = RustType.Opaque("Builder", builderModule.fullyQualifiedPath())
    return Symbol.builder()
        .rustType(rustType)
        .name(rustType.name)
        .module(builderModule)
        .build()
}
