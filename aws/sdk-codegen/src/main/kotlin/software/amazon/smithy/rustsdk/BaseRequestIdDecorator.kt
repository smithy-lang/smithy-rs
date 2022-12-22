/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.ErrorCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.ErrorSection
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait

/**
 * Base customization for adding a request ID (or extended request ID) to outputs and errors.
 */
abstract class BaseRequestIdDecorator : ClientCodegenDecorator {
    abstract val accessorFunctionName: String
    abstract val fieldName: String
    abstract fun accessorTrait(codegenContext: ClientCodegenContext): RuntimeType
    abstract fun applyToError(codegenContext: ClientCodegenContext): RuntimeType

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> = baseCustomizations + listOf(RequestIdOperationCustomization(codegenContext))

    override fun errorCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ErrorCustomization>,
    ): List<ErrorCustomization> =
        baseCustomizations + listOf(RequestIdErrorCustomization(codegenContext))

    override fun structureCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<StructureCustomization>,
    ): List<StructureCustomization> = baseCustomizations + listOf(RequestIdStructureCustomization(codegenContext))

    override fun builderCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<BuilderCustomization>,
    ): List<BuilderCustomization> = baseCustomizations + listOf(RequestIdBuilderCustomization())

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        rustCrate.withModule(RustModule.Types) {
            // Re-export RequestId in generated crate
            rust("pub use #T;", accessorTrait(codegenContext))
        }
    }

    private inner class RequestIdOperationCustomization(private val codegenContext: ClientCodegenContext) :
        OperationCustomization() {
        override fun section(section: OperationSection): Writable = writable {
            when (section) {
                is OperationSection.PopulateGenericErrorExtras -> {
                    rustTemplate(
                        "${section.builderName} = #{apply_to_error}(${section.builderName}, ${section.responseName}.headers());",
                        "apply_to_error" to applyToError(codegenContext),
                    )
                }
                is OperationSection.MutateOutput -> {
                    rust(
                        "output._set_$fieldName(#T::$accessorFunctionName(response).map(str::to_string));",
                        accessorTrait(codegenContext),
                    )
                }
                else -> {}
            }
        }
    }

    private inner class RequestIdErrorCustomization(private val codegenContext: ClientCodegenContext) :
        ErrorCustomization() {
        override fun section(section: ErrorSection): Writable = writable {
            when (section) {
                is ErrorSection.OperationErrorAdditionalTraitImpls -> {
                    rustTemplate(
                        """
                        impl #{AccessorTrait} for #{error} {
                            fn $accessorFunctionName(&self) -> Option<&str> {
                                self.meta.$accessorFunctionName()
                            }
                        }
                        """,
                        "AccessorTrait" to accessorTrait(codegenContext),
                        "error" to section.errorType,
                    )
                }

                else -> {}
            }
        }
    }

    private inner class RequestIdStructureCustomization(private val codegenContext: ClientCodegenContext) :
        StructureCustomization() {
        override fun section(section: StructureSection): Writable = writable {
            if (section.shape.hasTrait<SyntheticOutputTrait>()) {
                when (section) {
                    is StructureSection.AdditionalFields -> {
                        rust("_$fieldName: Option<String>,")
                    }

                    is StructureSection.AdditionalTraitImpls -> {
                        rustTemplate(
                            """
                            impl #{AccessorTrait} for ${section.structName} {
                                fn $accessorFunctionName(&self) -> Option<&str> {
                                    self._$fieldName.as_deref()
                                }
                            }
                            """,
                            "AccessorTrait" to accessorTrait(codegenContext),
                        )
                    }

                    is StructureSection.AdditionalDebugFields -> {
                        rust("""${section.formatterName}.field("_$fieldName", &self._$fieldName);""")
                    }
                }
            }
        }
    }

    private inner class RequestIdBuilderCustomization : BuilderCustomization() {
        override fun section(section: BuilderSection): Writable = writable {
            if (section.shape.hasTrait<SyntheticOutputTrait>()) {
                when (section) {
                    is BuilderSection.AdditionalFields -> {
                        rust("_$fieldName: Option<String>,")
                    }

                    is BuilderSection.AdditionalMethods -> {
                        rust(
                            """
                            pub(crate) fn _$fieldName(mut self, $fieldName: impl Into<String>) -> Self {
                                self._$fieldName = Some($fieldName.into());
                                self
                            }

                            pub(crate) fn _set_$fieldName(&mut self, $fieldName: Option<String>) -> &mut Self {
                                self._$fieldName = $fieldName;
                                self
                            }
                            """,
                        )
                    }

                    is BuilderSection.AdditionalDebugFields -> {
                        rust("""${section.formatterName}.field("_$fieldName", &self._$fieldName);""")
                    }

                    is BuilderSection.AdditionalFieldsInBuild -> {
                        rust("_$fieldName: self._$fieldName,")
                    }
                }
            }
        }
    }
}
