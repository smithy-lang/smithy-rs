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
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.docsOrFallback
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.makeOptional
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

/**
 * This decorator adds `ClientContextParams` to the service config.
 *
 * This handles injecting parameters like `s3::Accelerate` or `s3::ForcePathStyle`. The resulting parameters become
 * setters on the config builder object.
 */
internal class ClientContextDecorator(ctx: CodegenContext) : ConfigCustomization() {
    private val contextParams = ctx.serviceShape.getTrait<ClientContextParamsTrait>()?.parameters.orEmpty().toList()
        .map { (key, value) -> ContextParam.fromClientParam(key, value, ctx.symbolProvider) }

    data class ContextParam(val name: String, val type: Symbol, val docs: String?) {
        companion object {
            private fun toSymbol(shapeType: ShapeType, symbolProvider: RustSymbolProvider): Symbol =
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
            ): ContextParam {
                return ContextParam(
                    RustReservedWords.escapeIfNeeded(name.toSnakeCase()),
                    toSymbol(definition.type, symbolProvider),
                    definition.documentation.orNull(),
                )
            }
        }
    }

    override fun section(section: ServiceConfig): Writable {
        return when (section) {
            is ServiceConfig.ConfigStruct -> writable {
                contextParams.forEach { param ->
                    rust("pub (crate) ${param.name}: #T,", param.type.makeOptional())
                }
            }
            ServiceConfig.ConfigImpl -> emptySection
            ServiceConfig.BuilderStruct -> writable {
                contextParams.forEach { param ->
                    rust("${param.name}: #T,", param.type.makeOptional())
                }
            }
            ServiceConfig.BuilderImpl -> writable {
                contextParams.forEach { param ->
                    docsOrFallback(param.docs)
                    rust(
                        """
                        pub fn ${param.name}(mut self, ${param.name}: impl Into<#T>) -> Self {
                            self.${param.name} = Some(${param.name}.into());
                            self
                        }""",
                        param.type,
                    )

                    docsOrFallback(param.docs)
                    rust(
                        """
                        pub fn set_${param.name}(&mut self, ${param.name}: Option<#T>) -> &mut Self {
                            self.${param.name} = ${param.name};
                            self
                        }
                        """,
                        param.type,
                    )
                }
            }
            ServiceConfig.BuilderBuild -> writable {
                contextParams.forEach { param ->
                    rust("${param.name}: self.${param.name},")
                }
            }
            else -> emptySection
        }
    }
}
