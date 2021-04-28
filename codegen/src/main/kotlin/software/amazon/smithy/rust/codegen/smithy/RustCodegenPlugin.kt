/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.build.SmithyBuildPlugin
import software.amazon.smithy.model.Model
import software.amazon.smithy.rust.codegen.rustlang.RustReservedWordSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.customize.CombinedCodegenDecorator

class RustCodegenPlugin : SmithyBuildPlugin {
    override fun getName(): String = "rust-codegen"

    override fun execute(context: PluginContext) {
        val codegenDecorator = CombinedCodegenDecorator.fromClasspath(context)
        CodegenVisitor(context, codegenDecorator).execute()
    }

    companion object {
        fun BaseSymbolProvider(model: Model, symbolVisitorConfig: SymbolVisitorConfig = DefaultConfig) =
            SymbolVisitor(model, config = symbolVisitorConfig)
                .let { BaseSymbolMetadataProvider(it) }
                .let { RustReservedWordSymbolProvider(it) }
    }
}
