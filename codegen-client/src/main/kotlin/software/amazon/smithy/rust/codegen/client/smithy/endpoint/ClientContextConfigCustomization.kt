/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.ShapeType
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.rulesengine.traits.ClientContextParamDefinition
import software.amazon.smithy.rulesengine.traits.ClientContextParamsTrait
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigParam
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.standardConfigParam
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

/**
 * This decorator adds `ClientContextParams` to the service config.
 *
 * This handles injecting parameters like `s3::Accelerate` or `s3::ForcePathStyle`. The resulting parameters become
 * setters on the config builder object.
 */
class ClientContextConfigCustomization(ctx: CodegenContext) : ConfigCustomization() {
    private val configParams = ctx.serviceShape.getTrait<ClientContextParamsTrait>()?.parameters.orEmpty().toList()
        .map { (key, value) -> fromClientParam(key, value, ctx.symbolProvider) }
    private val decorators = configParams.map { standardConfigParam(it) }

    companion object {
        fun toSymbol(shapeType: ShapeType, symbolProvider: RustSymbolProvider): Symbol =
            symbolProvider.toSymbol(
                when (shapeType) {
                    ShapeType.STRING -> StringShape.builder().id("smithy.api#String").build()
                    ShapeType.BOOLEAN -> BooleanShape.builder().id("smithy.api#Boolean").build()
                    else -> TODO("unsupported type")
                },
            )

        fun fromClientParam(
            name: String,
            definition: ClientContextParamDefinition,
            symbolProvider: RustSymbolProvider,
        ): ConfigParam {
            return ConfigParam(
                RustReservedWords.escapeIfNeeded(name.toSnakeCase()),
                toSymbol(definition.type, symbolProvider),
                definition.documentation.orNull()?.let { writable { docs(it) } },
            )
        }
    }

    override fun section(section: ServiceConfig): Writable {
        return decorators.map { it.section(section) }.join("\n")
    }
}
