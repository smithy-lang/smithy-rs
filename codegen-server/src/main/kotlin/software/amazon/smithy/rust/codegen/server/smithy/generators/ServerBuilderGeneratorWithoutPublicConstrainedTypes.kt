/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.client.rustlang.Attribute
import software.amazon.smithy.rust.codegen.client.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.client.rustlang.conditionalBlock
import software.amazon.smithy.rust.codegen.client.rustlang.deprecatedShape
import software.amazon.smithy.rust.codegen.client.rustlang.docs
import software.amazon.smithy.rust.codegen.client.rustlang.documentShape
import software.amazon.smithy.rust.codegen.client.rustlang.rust
import software.amazon.smithy.rust.codegen.client.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.client.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.client.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.client.rustlang.withBlock
import software.amazon.smithy.rust.codegen.client.rustlang.writable
import software.amazon.smithy.rust.codegen.client.smithy.ConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.client.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.client.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.client.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.serverBuilderSymbol
import software.amazon.smithy.rust.codegen.client.smithy.isOptional
import software.amazon.smithy.rust.codegen.client.smithy.makeOptional
import software.amazon.smithy.rust.codegen.server.smithy.ServerRuntimeType

/**
 * TODO Docs
 * This builder only enforces the `required` trait.
 *
 * This builder is always public.
 */
class ServerBuilderGeneratorWithoutPublicConstrainedTypes(
    codegenContext: ServerCodegenContext,
    private val shape: StructureShape,
) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val constraintViolationSymbolProvider =
        ConstraintViolationSymbolProvider(codegenContext.symbolProvider, model, codegenContext.serviceShape)
    private val publicConstrainedTypes = codegenContext.settings.codegenConfig.publicConstrainedTypes
    private val members: List<MemberShape> = shape.allMembers.values.toList()
    private val structureSymbol = symbolProvider.toSymbol(shape)

    // TODO moduleName
    private val builderSymbol = shape.serverBuilderSymbol(symbolProvider, false)
    private val moduleName = builderSymbol.namespace.split("::").last()
    private val isBuilderFallible = StructureGenerator.serverHasFallibleBuilderWithoutPublicConstrainedTypes(shape, symbolProvider)

    private val codegenScope = arrayOf(
        "RequestRejection" to ServerRuntimeType.RequestRejection(codegenContext.runtimeConfig),
        "Structure" to structureSymbol,
        "From" to RuntimeType.From,
        "TryFrom" to RuntimeType.TryFrom,
        "MaybeConstrained" to RuntimeType.MaybeConstrained(),
    )

    fun render(writer: RustWriter) {
        writer.docs("See #D.", structureSymbol)
        writer.withModule(moduleName) {
            renderBuilder(this)
        }
    }

    private fun renderBuilder(writer: RustWriter) {
        if (isBuilderFallible) {
            Attribute.Derives(setOf(RuntimeType.Debug, RuntimeType.PartialEq)).render(writer)
            writer.docs("Holds one variant for each of the ways the builder can fail.")
            val constraintViolationSymbolName = constraintViolationSymbolProvider.toSymbol(shape).name
            writer.rustBlock("pub enum $constraintViolationSymbolName") {
                constraintViolations().forEach {
                    renderConstraintViolation(
                        this,
                        it,
                        model,
                        constraintViolationSymbolProvider,
                        symbolProvider,
                        structureSymbol,
                    )
                }
            }

            renderImplDisplayConstraintViolation(writer)
            writer.rust("impl #T for ConstraintViolation { }", RuntimeType.StdError)

            renderTryFromBuilderImpl(writer)
        } else {
            renderFromBuilderImpl(writer)
        }

        writer.docs("A builder for #D.", structureSymbol)
        // Matching derives to the main structure, - `PartialEq`, + `Default` since we are a builder and everything is optional.
        // TODO Manually implement `Default` so that we can add custom docs.
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

    // TODO Extract
    // TODO This impl does not take into account sensitive trait.
    private fun renderImplDisplayConstraintViolation(writer: RustWriter) {
        writer.rustBlock("impl #T for ConstraintViolation", RuntimeType.Display) {
            rustBlock("fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result") {
                rustBlock("match self") {
                    constraintViolations().forEach {
                        val arm = if (it.hasInner()) {
                            "ConstraintViolation::${it.name()}(_)"
                        } else {
                            "ConstraintViolation::${it.name()}"
                        }
                        rust("""$arm => write!(f, "${constraintViolationMessage(it, symbolProvider, structureSymbol)}"),""")
                    }
                }
            }
        }
    }

    // TODO Extract
    private fun buildFnReturnType() = writable {
        if (isBuilderFallible) {
            rust("Result<#T, ConstraintViolation>", structureSymbol)
        } else {
            rust("#T", structureSymbol)
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
        // TODO Could be single block
        implBlockWriter.rustBlockTemplate("pub fn build(self) -> #{ReturnType:W}", "ReturnType" to buildFnReturnType()) {
            rust("self.build_enforcing_required_and_enum_traits()")
        }
        renderBuildEnforcingRequiredAndEnumTraitsFn(implBlockWriter)
    }

    private fun renderBuildEnforcingRequiredAndEnumTraitsFn(implBlockWriter: RustWriter) {
        implBlockWriter.rustBlockTemplate("fn build_enforcing_required_and_enum_traits(self) -> #{ReturnType:W}", "ReturnType" to buildFnReturnType()) {
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
                    builderMissingFieldForMember(member, symbolProvider)?.also {
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

    private fun constraintViolations() = members.flatMap { member ->
        listOfNotNull(builderMissingFieldForMember(member, symbolProvider))
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
