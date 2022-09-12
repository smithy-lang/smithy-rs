/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Visibility
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.server.smithy.PubCrateConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.PubCrateConstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustBoxTrait
import software.amazon.smithy.rust.codegen.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.smithy.isDirectlyConstrained
import software.amazon.smithy.rust.codegen.smithy.letIf
import software.amazon.smithy.rust.codegen.smithy.makeMaybeConstrained
import software.amazon.smithy.rust.codegen.smithy.makeRustBoxed
import software.amazon.smithy.rust.codegen.smithy.targetCanReachConstrainedShape
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.toPascalCase

// TODO Docs
class UnconstrainedUnionGenerator(
    val codegenContext: ServerCodegenContext,
    private val pubCrateConstrainedShapeSymbolProvider: PubCrateConstrainedShapeSymbolProvider,
    private val unconstrainedModuleWriter: RustWriter,
    private val modelsModuleWriter: RustWriter,
    val shape: UnionShape
) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val unconstrainedShapeSymbolProvider = codegenContext.unconstrainedShapeSymbolProvider
    private val publicConstrainedTypes = codegenContext.settings.codegenConfig.publicConstrainedTypes
    private val constraintViolationSymbolProvider =
        with (codegenContext.constraintViolationSymbolProvider) {
            if (publicConstrainedTypes) {
                this
            } else {
                PubCrateConstraintViolationSymbolProvider(this)
            }
        }
    private val symbol = unconstrainedShapeSymbolProvider.toSymbol(shape)
    private val sortedMembers: List<MemberShape> = shape.allMembers.values.sortedBy { symbolProvider.toMemberName(it) }

    fun render() {
        check(shape.canReachConstrainedShape(model, symbolProvider))

        val module = symbol.namespace.split(symbol.namespaceDelimiter).last()
        val name = symbol.name
        val constrainedSymbol = pubCrateConstrainedShapeSymbolProvider.toSymbol(shape)
        val constraintViolationSymbol = constraintViolationSymbolProvider.toSymbol(shape)
        val constraintViolationName = constraintViolationSymbol.name

        unconstrainedModuleWriter.withModule(module, RustMetadata(visibility = Visibility.PUBCRATE)) {
            rustBlock(
                """
                ##[allow(clippy::enum_variant_names)]
                ##[derive(Debug, Clone)]
                pub(crate) enum $name
                """
            ) {
                sortedMembers.forEach { member ->
                    rust(
                        "${unconstrainedShapeSymbolProvider.toMemberName(member)}(#T),",
                        unconstrainedShapeSymbolProvider.toSymbol(member)
                    )
                }
            }

            rustTemplate(
                """
                impl #{TryFrom}<$name> for #{ConstrainedSymbol} {
                    type Error = #{ConstraintViolationSymbol};
                
                    fn try_from(value: $name) -> Result<Self, Self::Error> {
                        #{body:W}
                    }
                }
                """,
                "TryFrom" to RuntimeType.TryFrom,
                "ConstrainedSymbol" to constrainedSymbol,
                "ConstraintViolationSymbol" to constraintViolationSymbol,
                "body" to generateTryFromUnconstrainedUnionImpl(),
            )
        }

        modelsModuleWriter.rustTemplate(
            """
            impl #{ConstrainedTrait} for #{ConstrainedSymbol}  {
                type Unconstrained = #{UnconstrainedSymbol};
            }
            
            impl From<#{UnconstrainedSymbol}> for #{MaybeConstrained} {
                fn from(value: #{UnconstrainedSymbol}) -> Self {
                    Self::Unconstrained(value)
                }
            }
            """,
            "ConstrainedTrait" to RuntimeType.ConstrainedTrait(),
            "MaybeConstrained" to constrainedSymbol.makeMaybeConstrained(),
            "ConstrainedSymbol" to constrainedSymbol,
            "UnconstrainedSymbol" to symbol
        )

        val constraintViolationVisibility = if (publicConstrainedTypes) {
            Visibility.PUBLIC
        } else {
            Visibility.PUBCRATE
        }
        modelsModuleWriter.withModule(
            constraintViolationSymbol.namespace.split(constraintViolationSymbol.namespaceDelimiter).last(),
            RustMetadata(visibility = constraintViolationVisibility)
        ) {
            Attribute.Derives(setOf(RuntimeType.Debug, RuntimeType.PartialEq)).render(this)
            rustBlock("pub enum $constraintViolationName") {
                constraintViolations().forEach { renderConstraintViolation(this, it) }
            }
        }
    }

    data class ConstraintViolation(val forMember: MemberShape) {
        fun name() = forMember.memberName.toPascalCase()
    }

    private fun constraintViolations() =
        sortedMembers
            .filter { it.targetCanReachConstrainedShape(model, symbolProvider) }
            .map { ConstraintViolation(it) }

    private fun renderConstraintViolation(writer: RustWriter, constraintViolation: ConstraintViolation) {
        val targetShape = model.expectShape(constraintViolation.forMember.target)

        val constraintViolationSymbol =
            constraintViolationSymbolProvider.toSymbol(targetShape)
                // If the corresponding union's member is boxed, box this constraint violation symbol too.
                .letIf(constraintViolation.forMember.hasTrait<RustBoxTrait>()) {
                    it.makeRustBoxed()
                }

        writer.rust(
            "${constraintViolation.name()}(#T),",
            constraintViolationSymbol
        )
    }

    private fun generateTryFromUnconstrainedUnionImpl() = writable {
        withBlock("Ok(", ")") {
            withBlock("match value {", "}") {
                sortedMembers.forEach { member ->
                    val memberName = unconstrainedShapeSymbolProvider.toMemberName(member)
                    withBlockTemplate(
                        "#{UnconstrainedUnion}::$memberName(unconstrained) => Self::$memberName({",
                        "}),",
                        "UnconstrainedUnion" to symbol,
                    ) {
                        if (member.targetCanReachConstrainedShape(model, symbolProvider)) {
                            val targetShape = model.expectShape(member.target)
                            val resolveToNonPublicConstrainedType =
                                targetShape !is StructureShape && targetShape !is UnionShape && !targetShape.hasTrait<EnumTrait>()
                                (!publicConstrainedTypes || !targetShape.isDirectlyConstrained(symbolProvider))

                            val hasBox = member.hasTrait<RustBoxTrait>()
                            if (hasBox) {
                                rust("let unconstrained = *unconstrained;")
                            }

                            if (resolveToNonPublicConstrainedType) {
                                val constrainedSymbol =
                                    if (!publicConstrainedTypes && targetShape.isDirectlyConstrained(symbolProvider)) {
                                        codegenContext.constrainedShapeSymbolProvider.toSymbol(targetShape)
                                    } else {
                                        pubCrateConstrainedShapeSymbolProvider.toSymbol(targetShape)
                                    }
                                rustTemplate(
                                    """
                                    let constrained: #{ConstrainedSymbol} = unconstrained
                                        .try_into()
                                        ${ if (hasBox) ".map(Box::new).map_err(Box::new)" else "" }
                                        .map_err(Self::Error::${ConstraintViolation(member).name()})?;
                                    constrained.into()
                                    """,
                                    "ConstrainedSymbol" to constrainedSymbol
                                )
                            } else {
                                rust(
                                    """
                                    unconstrained
                                        .try_into()
                                        ${ if (hasBox) ".map(Box::new).map_err(Box::new)" else "" }
                                        .map_err(Self::Error::${ConstraintViolation(member).name()})?
                                    """
                                )
                            }
                        } else {
                            rust("unconstrained")
                        }
                    }
                }
            }
        }
    }
}
