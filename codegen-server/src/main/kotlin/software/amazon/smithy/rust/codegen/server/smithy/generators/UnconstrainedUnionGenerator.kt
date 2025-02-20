/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.derive
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.makeMaybeConstrained
import software.amazon.smithy.rust.codegen.core.smithy.makeRustBoxed
import software.amazon.smithy.rust.codegen.core.smithy.traits.RustBoxTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.isTargetUnit
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.server.smithy.InlineModuleCreator
import software.amazon.smithy.rust.codegen.server.smithy.PubCrateConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.server.smithy.isDirectlyConstrained
import software.amazon.smithy.rust.codegen.server.smithy.targetCanReachConstrainedShape
import software.amazon.smithy.rust.codegen.server.smithy.traits.ConstraintViolationRustBoxTrait
import software.amazon.smithy.rust.codegen.server.smithy.traits.isReachableFromOperationInput

/**
 * Generates a Rust type for a constrained union shape that is able to hold values for the corresponding _unconstrained_
 * shape. This type is a [RustType.Opaque] enum newtype, with each variant holding the corresponding unconstrained type.
 * Upon request parsing, server deserializers use this type to store the incoming values without enforcing the modeled
 * constraints. Only after the full request has been parsed are constraints enforced, via the `impl
 * TryFrom<UnconstrainedSymbol> for ConstrainedSymbol`.
 *
 * This type is never exposed to the user; it is always `pub(crate)`. Only the deserializers use it.
 *
 * Consult [UnconstrainedShapeSymbolProvider] for more details and for an example.
 */
class UnconstrainedUnionGenerator(
    val codegenContext: ServerCodegenContext,
    private val inlineModuleCreator: InlineModuleCreator,
    private val modelsModuleWriter: RustWriter,
    val shape: UnionShape,
    private val validationExceptionConversionGenerator: ValidationExceptionConversionGenerator,
) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val pubCrateConstrainedShapeSymbolProvider = codegenContext.pubCrateConstrainedShapeSymbolProvider
    private val unconstrainedShapeSymbolProvider = codegenContext.unconstrainedShapeSymbolProvider
    private val publicConstrainedTypes = codegenContext.settings.codegenConfig.publicConstrainedTypes
    private val constraintViolationSymbolProvider =
        with(codegenContext.constraintViolationSymbolProvider) {
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

        val name = symbol.name
        val constrainedSymbol = pubCrateConstrainedShapeSymbolProvider.toSymbol(shape)
        val constraintViolationSymbol = constraintViolationSymbolProvider.toSymbol(shape)
        val constraintViolationName = constraintViolationSymbol.name

        inlineModuleCreator(symbol) {
            rustBlock(
                """
                ##[allow(clippy::enum_variant_names)]
                ##[derive(Debug, Clone)]
                pub(crate) enum $name
                """,
            ) {
                sortedMembers.forEach { member ->
                    if (member.isTargetUnit()) {
                        rust(
                            "${unconstrainedShapeSymbolProvider.toMemberName(member)},",
                        )
                    } else {
                        rust(
                            "${unconstrainedShapeSymbolProvider.toMemberName(member)}(#T),",
                            unconstrainedShapeSymbolProvider.toSymbol(member),
                        )
                    }
                }
            }

            rustTemplate(
                """
                impl #{TryFrom}<$name> for #{ConstrainedSymbol} {
                    type Error = #{ConstraintViolationSymbol};

                    fn try_from(value: $name) -> #{Result}<Self, Self::Error> {
                        #{body:W}
                    }
                }
                """,
                "TryFrom" to RuntimeType.TryFrom,
                "ConstrainedSymbol" to constrainedSymbol,
                "ConstraintViolationSymbol" to constraintViolationSymbol,
                "body" to generateTryFromUnconstrainedUnionImpl(),
                *RuntimeType.preludeScope,
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
            "ConstrainedTrait" to RuntimeType.ConstrainedTrait,
            "MaybeConstrained" to constrainedSymbol.makeMaybeConstrained(),
            "ConstrainedSymbol" to constrainedSymbol,
            "UnconstrainedSymbol" to symbol,
        )

        val constraintViolationVisibility =
            if (publicConstrainedTypes) {
                Visibility.PUBLIC
            } else {
                Visibility.PUBCRATE
            }

        inlineModuleCreator(
            constraintViolationSymbol,
        ) {
            Attribute(derive(RuntimeType.Debug, RuntimeType.PartialEq)).render(this)
            rustBlock(
                """
                ##[allow(clippy::enum_variant_names)]
                pub${if (constraintViolationVisibility == Visibility.PUBCRATE) " (crate)" else ""} enum $constraintViolationName""",
            ) {
                constraintViolations().forEach { renderConstraintViolation(this, it) }
            }

            rustTemplate(
                """
                impl #{Display} for $constraintViolationName {
                    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        match self {
                            #{ConstraintVariants:W}
                        }
                    }
                }

                impl #{Error} for $constraintViolationName {}
                """,
                "Error" to RuntimeType.StdError,
                "Display" to RuntimeType.Display,
                "ConstraintVariants" to generateDisplayMessageForEachVariant(),
            )

            if (shape.isReachableFromOperationInput()) {
                rustTemplate(
                    """
                    impl $constraintViolationName {
                        #{UnionShapeConstraintViolationImplBlock:W}
                    }
                    """,
                    "UnionShapeConstraintViolationImplBlock" to
                        validationExceptionConversionGenerator.unionShapeConstraintViolationImplBlock(constraintViolations()),
                )
            }
        }
    }

    private fun generateDisplayMessageForEachVariant() =
        writable {
            constraintViolations().forEach {
                rustTemplate(
                    """
                    Self::${it.name()}(inner) => write!(f, "{inner}"),
                    """,
                )
            }
        }

    private fun constraintViolations() =
        sortedMembers
            .filter { it.targetCanReachConstrainedShape(model, symbolProvider) }
            .map { UnionConstraintTraitInfo(it) }

    private fun renderConstraintViolation(
        writer: RustWriter,
        unionConstraintTraitInfo: UnionConstraintTraitInfo,
    ) {
        val targetShape = model.expectShape(unionConstraintTraitInfo.forMember.target)

        val constraintViolationSymbol =
            constraintViolationSymbolProvider.toSymbol(targetShape)
                // Box this constraint violation symbol if necessary.
                .letIf(unionConstraintTraitInfo.forMember.hasTrait<ConstraintViolationRustBoxTrait>()) {
                    it.makeRustBoxed()
                }

        writer.rust(
            "${unionConstraintTraitInfo.name()}(#T),",
            constraintViolationSymbol,
        )
    }

    private fun generateTryFromUnconstrainedUnionImpl() =
        writable {
            withBlock("Ok(", ")") {
                withBlock("match value {", "}") {
                    sortedMembers.forEach { member ->
                        val memberName = unconstrainedShapeSymbolProvider.toMemberName(member)
                        if (member.isTargetUnit()) {
                            // Unit type within Unions do not have associated data.
                            rustTemplate(
                                """
                                #{UnconstrainedUnion}::$memberName => Self::$memberName,
                                """,
                                "UnconstrainedUnion" to symbol,
                            )
                        } else {
                            withBlockTemplate(
                                "#{UnconstrainedUnion}::$memberName(unconstrained) => Self::$memberName(",
                                "),",
                                "UnconstrainedUnion" to symbol,
                            ) {
                                if (!member.canReachConstrainedShape(model, symbolProvider)) {
                                    rust("unconstrained")
                                } else {
                                    generateTryFromImplForReachableConstrainedShape(member).invoke(this)
                                }
                            }
                        }
                    }
                }
            }
        }

    private fun generateTryFromImplForReachableConstrainedShape(member: MemberShape) =
        writable {
            val targetShape = model.expectShape(member.target)
            val resolveToNonPublicConstrainedType =
                targetShape !is StructureShape && targetShape !is UnionShape && !targetShape.hasTrait<EnumTrait>() &&
                    (!publicConstrainedTypes || !targetShape.isDirectlyConstrained(symbolProvider))

            val (unconstrainedVar, boxIt) =
                if (member.hasTrait<RustBoxTrait>()) {
                    "(*unconstrained)" to ".map(Box::new)"
                } else {
                    "unconstrained" to ""
                }
            val boxErr =
                if (member.hasTrait<ConstraintViolationRustBoxTrait>()) {
                    ".map_err(Box::new)"
                } else {
                    ""
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
                    {
                        let constrained: #{ConstrainedSymbol} = $unconstrainedVar
                            .try_into()$boxIt$boxErr
                            .map_err(Self::Error::${UnionConstraintTraitInfo(member).name()})?;
                        constrained.into()
                    }
                    """,
                    "ConstrainedSymbol" to constrainedSymbol,
                )
            } else {
                rust(
                    """
                    $unconstrainedVar
                        .try_into()
                        $boxIt
                        $boxErr
                        .map_err(Self::Error::${UnionConstraintTraitInfo(member).name()})?
                    """,
                )
            }
        }
}

data class UnionConstraintTraitInfo(val forMember: MemberShape) {
    fun name() = forMember.memberName.toPascalCase()
}
