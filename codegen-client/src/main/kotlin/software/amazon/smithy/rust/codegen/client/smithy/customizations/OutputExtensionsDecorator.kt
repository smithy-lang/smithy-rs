/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureSection
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait

/**
 * Adds an [`Extensions`] type-map to every operation output, accessible through the
 * `ProvideExtensions` trait. Extensions carry auxiliary data attached during request
 * processing (for example response checksum validation results) that is not part of a
 * modeled shape.
 *
 * The field is held in an inlined `EqIgnore` wrapper so it does not participate in the
 * output's derived `PartialEq` (`Extensions` has no meaningful equality), while the
 * output keeps its blanket derives.
 *
 * This decorator only provides the container and accessor. Population is generic: the
 * generated response deserializer lifts the `Extensions` accumulated in the config bag
 * onto the output via `_set_extensions`. Feature-specific interceptors contribute by
 * inserting their typed handles into the config-bag `Extensions`.
 */
class OutputExtensionsDecorator : ClientCodegenDecorator {
    override val name: String = "OutputExtensions"
    override val order: Byte = 0

    private fun extensionsType(codegenContext: ClientCodegenContext): RuntimeType =
        RuntimeType.smithyTypes(codegenContext.runtimeConfig).resolve("extensions::Extensions")

    private fun provideExtensionsTrait(codegenContext: ClientCodegenContext): RuntimeType =
        RuntimeType.smithyTypes(codegenContext.runtimeConfig).resolve("extensions::ProvideExtensions")

    private fun eqIgnore(codegenContext: ClientCodegenContext): RuntimeType =
        RuntimeType.forInlineDependency(
            InlineDependency.forRustFile(
                RustModule.pubCrate("eq_ignore", parent = ClientRustModule.root),
                "/inlineable/src/eq_ignore.rs",
            ),
        ).resolve("EqIgnore")

    override fun structureCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<StructureCustomization>,
    ): List<StructureCustomization> = baseCustomizations + OutputExtensionsStructureCustomization(codegenContext)

    override fun builderCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<BuilderCustomization>,
    ): List<BuilderCustomization> = baseCustomizations + OutputExtensionsBuilderCustomization(codegenContext)

    override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        rustCrate.withModule(ClientRustModule.Operation) {
            // Re-export ProvideExtensions in the generated crate.
            rust("pub use #T;", provideExtensionsTrait(codegenContext))
        }
    }

    private inner class OutputExtensionsStructureCustomization(
        private val codegenContext: ClientCodegenContext,
    ) : StructureCustomization() {
        override fun section(section: StructureSection): Writable =
            writable {
                if (!section.shape.hasTrait<SyntheticOutputTrait>()) {
                    return@writable
                }
                when (section) {
                    is StructureSection.AdditionalFields -> {
                        rustTemplate(
                            "_extensions: #{EqIgnore}<#{Extensions}>,",
                            "EqIgnore" to eqIgnore(codegenContext),
                            "Extensions" to extensionsType(codegenContext),
                        )
                    }

                    is StructureSection.AdditionalTraitImpls -> {
                        rustTemplate(
                            """
                            impl #{ProvideExtensions} for ${section.structName} {
                                fn extensions(&self) -> &#{Extensions} {
                                    self._extensions.get()
                                }
                            }

                            impl ${section.structName} {
                                /// Replaces this output's extensions. Used by the generated response
                                /// deserializer to lift extensions accumulated in the config bag onto
                                /// the output.
                                ##[allow(dead_code)]
                                pub(crate) fn _set_extensions(&mut self, extensions: #{Extensions}) {
                                    *self._extensions.get_mut() = extensions;
                                }
                            }
                            """,
                            "ProvideExtensions" to provideExtensionsTrait(codegenContext),
                            "Extensions" to extensionsType(codegenContext),
                        )
                    }

                    is StructureSection.AdditionalDebugFields -> {
                        rust("""${section.formatterName}.field("_extensions", &self._extensions);""")
                    }
                }
            }
    }

    private inner class OutputExtensionsBuilderCustomization(
        private val codegenContext: ClientCodegenContext,
    ) : BuilderCustomization() {
        override fun section(section: BuilderSection): Writable =
            writable {
                if (!section.shape.hasTrait<SyntheticOutputTrait>()) {
                    return@writable
                }
                when (section) {
                    is BuilderSection.AdditionalFields -> {
                        rustTemplate(
                            "_extensions: #{EqIgnore}<#{Extensions}>,",
                            "EqIgnore" to eqIgnore(codegenContext),
                            "Extensions" to extensionsType(codegenContext),
                        )
                    }

                    is BuilderSection.AdditionalDebugFields -> {
                        rust("""${section.formatterName}.field("_extensions", &self._extensions);""")
                    }

                    is BuilderSection.AdditionalFieldsInBuild -> {
                        rust("_extensions: self._extensions,")
                    }

                    else -> {}
                }
            }
    }
}
