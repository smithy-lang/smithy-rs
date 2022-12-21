/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.conditionalBlock
import software.amazon.smithy.rust.codegen.core.rustlang.deprecatedShape
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.documentShape
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.makeOptional
import software.amazon.smithy.rust.codegen.core.smithy.module
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.ServerRuntimeType

/**
 * Generates a builder for the Rust type associated with the [StructureShape].
 *
 * This builder is similar in design to [ServerBuilderGenerator], so consult its documentation in that regard. However,
 * this builder has a few differences.
 *
 * Unlike [ServerBuilderGenerator], this builder only enforces constraints that are baked into the type system _when
 * `publicConstrainedTypes` is false_. So in terms of honoring the Smithy spec, this builder only enforces enums
 * and the `required` trait.
 *
 * Unlike [ServerBuilderGenerator], this builder is always public. It is the only builder type the user is exposed to
 * when `publicConstrainedTypes` is false.
 */
class ServerBuilderGeneratorWithoutPublicConstrainedTypes(
    codegenContext: ServerCodegenContext,
    shape: StructureShape,
) {
    companion object {
        /**
         * Returns whether a structure shape, whose builder has been generated with
         * [ServerBuilderGeneratorWithoutPublicConstrainedTypes], requires a fallible builder to be constructed.
         *
         * This builder only enforces the `required` trait.
         */
        fun hasFallibleBuilder(
            structureShape: StructureShape,
            symbolProvider: SymbolProvider,
        ): Boolean =
            structureShape
                .members()
                .map { symbolProvider.toSymbol(it) }
                .any { !it.isOptional() }
    }

    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val members: List<MemberShape> = shape.allMembers.values.toList()
    private val structureSymbol = symbolProvider.toSymbol(shape)

    private val builderSymbol = shape.serverBuilderSymbol(symbolProvider, false)
    private val moduleName = builderSymbol.namespace.split("::").last()
    private val isBuilderFallible = hasFallibleBuilder(shape, symbolProvider)
    private val serverBuilderConstraintViolations =
        ServerBuilderConstraintViolations(codegenContext, shape, builderTakesInUnconstrainedTypes = false)

    private val codegenScope = arrayOf(
        "RequestRejection" to ServerRuntimeType.requestRejection(codegenContext.runtimeConfig),
        "Structure" to structureSymbol,
        "From" to RuntimeType.From,
        "TryFrom" to RuntimeType.TryFrom,
        "MaybeConstrained" to RuntimeType.MaybeConstrained,
    )

    fun render(writer: RustWriter) {
        writer.docs("See #D.", structureSymbol)
        writer.withInlineModule(builderSymbol.module()) {
            renderBuilder(this)
        }
    }

    private fun renderBuilder(writer: RustWriter) {
        if (isBuilderFallible) {
            serverBuilderConstraintViolations.render(
                writer,
                Visibility.PUBLIC,
                nonExhaustive = false,
                shouldRenderAsValidationExceptionFieldList = false,
            )

            renderTryFromBuilderImpl(writer)
        } else {
            renderFromBuilderImpl(writer)
        }

        writer.docs("A builder for #D.", structureSymbol)
        // Matching derives to the main structure, - `PartialEq` (to be consistent with [ServerBuilderGenerator]), + `Default`
        // since we are a builder and everything is optional.
        val baseDerives = structureSymbol.expectRustMetadata().derives
        val derives = baseDerives.derives.intersect(setOf(RuntimeType.Debug, RuntimeType.Clone)) + RuntimeType.Default
        baseDerives.copy(derives = derives).render(writer)
        writer.rustBlock("pub struct Builder") {
            members.forEach { renderBuilderMember(this, it) }
        }

        writer.rustBlock("impl Builder") {
            for (member in members) {
                renderBuilderMemberFn(this, member)
            }
            renderBuildFn(this)
        }
    }

    private fun renderBuildFn(implBlockWriter: RustWriter) {
        implBlockWriter.docs("""Consumes the builder and constructs a #D.""", structureSymbol)
        if (isBuilderFallible) {
            implBlockWriter.docs(
                """
                The builder fails to construct a #D if you do not provide a value for all non-`Option`al members.
                """,
                structureSymbol,
            )
        }
        implBlockWriter.rustTemplate(
            """
            pub fn build(self) -> #{ReturnType:W} {
                self.build_enforcing_required_and_enum_traits()
            }
            """,
            "ReturnType" to buildFnReturnType(isBuilderFallible, structureSymbol),
        )
        renderBuildEnforcingRequiredAndEnumTraitsFn(implBlockWriter)
    }

    private fun renderBuildEnforcingRequiredAndEnumTraitsFn(implBlockWriter: RustWriter) {
        implBlockWriter.rustBlockTemplate(
            "fn build_enforcing_required_and_enum_traits(self) -> #{ReturnType:W}",
            "ReturnType" to buildFnReturnType(isBuilderFallible, structureSymbol),
        ) {
            conditionalBlock("Ok(", ")", conditional = isBuilderFallible) {
                coreBuilder(this)
            }
        }
    }

    private fun coreBuilder(writer: RustWriter) {
        writer.rustBlock("#T", structureSymbol) {
            for (member in members) {
                val memberName = symbolProvider.toMemberName(member)

                withBlock("$memberName: self.$memberName", ",") {
                    serverBuilderConstraintViolations.forMember(member)?.also {
                        rust(".ok_or(ConstraintViolation::${it.name()})?")
                    }
                }
            }
        }
    }

    fun renderConvenienceMethod(implBlock: RustWriter) {
        implBlock.docs("Creates a new builder-style object to manufacture #D.", structureSymbol)
        implBlock.rustBlock("pub fn builder() -> #T", builderSymbol) {
            write("#T::default()", builderSymbol)
        }
    }

    private fun renderBuilderMember(writer: RustWriter, member: MemberShape) {
        val memberSymbol = builderMemberSymbol(member)
        val memberName = symbolProvider.toMemberName(member)
        // Builder members are crate-public to enable using them directly in serializers/deserializers.
        // During XML deserialization, `builder.<field>.take` is used to append to lists and maps.
        writer.write("pub(crate) $memberName: #T,", memberSymbol)
    }

    /**
     * Render a `foo` method to set shape member `foo`. The caller must provide a value with the exact same type
     * as the shape member's type.
     *
     * This method is meant for use by the user; it is not used by the generated crate's (de)serializers.
     */
    private fun renderBuilderMemberFn(writer: RustWriter, member: MemberShape) {
        val memberSymbol = symbolProvider.toSymbol(member)
        val memberName = symbolProvider.toMemberName(member)

        writer.documentShape(member, model)
        writer.deprecatedShape(member)

        writer.rustBlock("pub fn $memberName(mut self, input: #T) -> Self", memberSymbol) {
            withBlock("self.$memberName = ", "; self") {
                conditionalBlock("Some(", ")", conditional = !memberSymbol.isOptional()) {
                    rust("input")
                }
            }
        }
    }

    private fun renderTryFromBuilderImpl(writer: RustWriter) {
        writer.rustTemplate(
            """
            impl #{TryFrom}<Builder> for #{Structure} {
                type Error = ConstraintViolation;

                fn try_from(builder: Builder) -> Result<Self, Self::Error> {
                    builder.build()
                }
            }
            """,
            *codegenScope,
        )
    }

    private fun renderFromBuilderImpl(writer: RustWriter) {
        writer.rustTemplate(
            """
            impl #{From}<Builder> for #{Structure} {
                fn from(builder: Builder) -> Self {
                    builder.build()
                }
            }
            """,
            *codegenScope,
        )
    }

    /**
     * Returns the symbol for a builder's member.
     */
    private fun builderMemberSymbol(member: MemberShape): Symbol = symbolProvider.toSymbol(member).makeOptional()
}
