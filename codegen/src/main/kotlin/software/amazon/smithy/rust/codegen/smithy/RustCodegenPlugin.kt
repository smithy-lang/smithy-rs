/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.build.SmithyBuildPlugin
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.rustlang.RustReservedWordSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.customize.CombinedCodegenDecorator

class RustCodegenPlugin : SmithyBuildPlugin {
    override fun getName(): String = "rust-codegen"

    override fun execute(context: PluginContext) {
        val codegenDecorator = CombinedCodegenDecorator.fromClasspath(context)
        CodegenVisitor(context, codegenDecorator).execute()
    }

    companion object {
        fun baseSymbolProvider(model: Model, serviceShape: ServiceShape, symbolVisitorConfig: SymbolVisitorConfig = DefaultConfig) =
            SymbolVisitor(model, serviceShape = serviceShape, config = symbolVisitorConfig)
                .let { StreamingShapeSymbolProvider(it, model) }
                .let { BaseSymbolMetadataProvider(it) }
                .let { StreamingShapeMetadataProvider(it, model) }
                .let { RustReservedWordSymbolProvider(it) }
    }
}
