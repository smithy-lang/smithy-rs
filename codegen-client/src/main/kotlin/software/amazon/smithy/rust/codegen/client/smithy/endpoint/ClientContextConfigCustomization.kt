/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.ShapeType
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.rulesengine.traits.ClientContextParamDefinition
import software.amazon.smithy.rulesengine.traits.ClientContextParamsTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigParam
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.configParamNewtype
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.standardConfigParam
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

/**
 * This decorator adds `ClientContextParams` to the service config.
 *
 * This handles injecting parameters like `s3::Accelerate` or `s3::ForcePathStyle`. The resulting parameters become
 * setters on the config builder object.
 */
class ClientContextConfigCustomization(ctx: ClientCodegenContext) : ConfigCustomization() {
    private val runtimeConfig = ctx.runtimeConfig
    private val configParams =
        ctx.serviceShape.getTrait<ClientContextParamsTrait>()?.parameters.orEmpty().toList()
            .map { (key, value) -> fromClientParam(key, value, ctx.symbolProvider, runtimeConfig) }
    private val decorators = configParams.map { standardConfigParam(it) }

    companion object {
        fun toSymbol(
            shapeType: ShapeType,
            symbolProvider: RustSymbolProvider,
        ): Symbol =
            symbolProvider.toSymbol(
                when (shapeType) {
                    ShapeType.STRING -> StringShape.builder().id("smithy.api#String").build()
                    ShapeType.BOOLEAN -> BooleanShape.builder().id("smithy.api#Boolean").build()
                    ShapeType.LIST -> ListShape.builder().id("smithy.api#List").build()
                    else -> TODO("unsupported type")
                },
            )

        fun fromClientParam(
            name: String,
            definition: ClientContextParamDefinition,
            symbolProvider: RustSymbolProvider,
            runtimeConfig: RuntimeConfig,
        ): ConfigParam {
            val inner = toSymbol(definition.type, symbolProvider)
            return ConfigParam(
                RustReservedWords.escapeIfNeeded(name.toSnakeCase()),
                inner,
                configParamNewtype(RustReservedWords.escapeIfNeeded(name.toPascalCase()), inner, runtimeConfig),
                definition.documentation.orNull()?.let { writable { docs(it) } },
            )
        }
    }

    override fun section(section: ServiceConfig): Writable {
        return decorators.map { it.section(section) }.join("\n")
    }
}
