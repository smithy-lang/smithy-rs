/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.error

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.implBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.genericError
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.ErrorImplCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.ErrorImplGenerator

class ErrorGenerator(
    private val model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val writer: RustWriter,
    private val shape: StructureShape,
    private val error: ErrorTrait,
    private val implCustomizations: List<ErrorImplCustomization>,
) {
    private val runtimeConfig = symbolProvider.config().runtimeConfig

    fun render() {
        val symbol = symbolProvider.toSymbol(shape)

        StructureGenerator(
            model, symbolProvider, writer, shape,
            listOf(object : StructureCustomization() {
                override fun section(section: StructureSection): Writable = writable {
                    when (section) {
                        is StructureSection.AdditionalFields -> {
                            rust("pub(crate) _meta: #T,", genericError(runtimeConfig))
                        }
                        is StructureSection.AdditionalDebugFields -> {
                            rust("""${section.formatterName}.field("_meta", &self._meta);""")
                        }
                    }
                }
            },
            ),
        ).render()

        BuilderGenerator(
            model, symbolProvider, shape,
            listOf(
                object : BuilderCustomization() {
                    override fun section(section: BuilderSection): Writable = writable {
                        when (section) {
                            is BuilderSection.AdditionalFields -> {
                                rust("_meta: Option<#T>,", genericError(runtimeConfig))
                            }

                            is BuilderSection.AdditionalMethods -> {
                                rustTemplate(
                                    """
                                    ##[doc(hidden)]
                                    pub fn _meta(mut self, _meta: #{generic_error}) -> Self {
                                        self._meta = Some(_meta);
                                        self
                                    }

                                    ##[doc(hidden)]
                                    pub fn _set_meta(&mut self, _meta: Option<#{generic_error}>) -> &mut Self {
                                        self._meta = _meta;
                                        self
                                    }
                                    """,
                                    "generic_error" to genericError(runtimeConfig),
                                )
                            }

                            is BuilderSection.AdditionalFieldsInBuild -> {
                                rust("_meta: self._meta.unwrap_or_default(),")
                            }
                        }
                    }
                },
            ),
        ).let { builderGen ->
            writer.implBlock(symbol) {
                builderGen.renderConvenienceMethod(this)
            }
            builderGen.render(writer)
        }

        ErrorImplGenerator(model, symbolProvider, writer, shape, error, implCustomizations).render(CodegenTarget.CLIENT)

        writer.rustBlock("impl #T for ${symbol.name}", RuntimeType.errorMetadataTrait(runtimeConfig)) {
            rust("fn meta(&self) -> &#T { &self._meta }", genericError(runtimeConfig))
        }
    }
}
