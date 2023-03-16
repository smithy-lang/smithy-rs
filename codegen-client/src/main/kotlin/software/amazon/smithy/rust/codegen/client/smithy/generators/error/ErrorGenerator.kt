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
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.errorMetadata
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
    private val shape: StructureShape,
    private val error: ErrorTrait,
    private val implCustomizations: List<ErrorImplCustomization>,
) {
    private val runtimeConfig = symbolProvider.config.runtimeConfig
    private val symbol = symbolProvider.toSymbol(shape)

    fun renderStruct(writer: RustWriter) {
        writer.apply {
            StructureGenerator(
                model, symbolProvider, this, shape,
                listOf(
                    object : StructureCustomization() {
                        override fun section(section: StructureSection): Writable = writable {
                            when (section) {
                                is StructureSection.AdditionalFields -> {
                                    rust("pub(crate) meta: #T,", errorMetadata(runtimeConfig))
                                }

                                is StructureSection.AdditionalDebugFields -> {
                                    rust("""${section.formatterName}.field("meta", &self.meta);""")
                                }

                                else -> {}
                            }
                        }
                    },
                ),
            ).render()

            ErrorImplGenerator(
                model,
                symbolProvider,
                this,
                shape,
                error,
                implCustomizations,
            ).render(CodegenTarget.CLIENT)

            rustBlock("impl #T for ${symbol.name}", RuntimeType.provideErrorMetadataTrait(runtimeConfig)) {
                rust("fn meta(&self) -> &#T { &self.meta }", errorMetadata(runtimeConfig))
            }

            implBlock(symbol) {
                BuilderGenerator.renderConvenienceMethod(this, symbolProvider, shape)
            }
        }
    }

    fun renderBuilder(writer: RustWriter) {
        writer.apply {
            BuilderGenerator(
                model, symbolProvider, shape,
                listOf(
                    object : BuilderCustomization() {
                        override fun section(section: BuilderSection): Writable = writable {
                            when (section) {
                                is BuilderSection.AdditionalFields -> {
                                    rust("meta: std::option::Option<#T>,", errorMetadata(runtimeConfig))
                                }

                                is BuilderSection.AdditionalMethods -> {
                                    rustTemplate(
                                        """
                                        /// Sets error metadata
                                        pub fn meta(mut self, meta: #{error_metadata}) -> Self {
                                            self.meta = Some(meta);
                                            self
                                        }

                                        /// Sets error metadata
                                        pub fn set_meta(&mut self, meta: std::option::Option<#{error_metadata}>) -> &mut Self {
                                            self.meta = meta;
                                            self
                                        }
                                        """,
                                        "error_metadata" to errorMetadata(runtimeConfig),
                                    )
                                }

                                is BuilderSection.AdditionalFieldsInBuild -> {
                                    rust("meta: self.meta.unwrap_or_default(),")
                                }

                                else -> {}
                            }
                        }
                    },
                ),
            ).render(this)
        }
    }
}
