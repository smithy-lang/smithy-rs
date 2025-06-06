/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.derive
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.makeRustBoxed
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.server.smithy.PubCrateConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.targetCanReachConstrainedShape
import software.amazon.smithy.rust.codegen.server.smithy.traits.ConstraintViolationRustBoxTrait

/**
 * Renders constraint violation types that arise when building a structure shape builder.
 *
 * Used by [ServerBuilderGenerator] and [ServerBuilderGeneratorWithoutPublicConstrainedTypes].
 */
class ServerBuilderConstraintViolations(
    codegenContext: ServerCodegenContext,
    private val shape: StructureShape,
    private val builderTakesInUnconstrainedTypes: Boolean,
    private val validationExceptionConversionGenerator: ValidationExceptionConversionGenerator,
) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val constraintViolationSymbolProvider =
        with(codegenContext.constraintViolationSymbolProvider) {
            if (codegenContext.settings.codegenConfig.publicConstrainedTypes) {
                this
            } else {
                PubCrateConstraintViolationSymbolProvider(this)
            }
        }
    private val members: List<MemberShape> = shape.allMembers.values.toList()
    val all =
        members.flatMap { member ->
            listOfNotNull(
                forMember(member),
                builderConstraintViolationForMember(member),
            )
        }

    fun render(
        writer: RustWriter,
        visibility: Visibility,
        nonExhaustive: Boolean,
        shouldRenderAsValidationExceptionFieldList: Boolean,
    ) {
        check(all.isNotEmpty()) {
            "Attempted to render constraint violations for the builder for structure shape ${shape.id}, but calculation of the constraint violations resulted in no variants"
        }

        Attribute(derive(RuntimeType.Debug, RuntimeType.PartialEq)).render(writer)
        writer.docs("Holds one variant for each of the ways the builder can fail.")
        if (nonExhaustive) Attribute.NonExhaustive.render(writer)
        val constraintViolationSymbolName = constraintViolationSymbolProvider.toSymbol(shape).name
        writer.rustBlock(
            """##[allow(clippy::enum_variant_names)]
            pub${if (visibility == Visibility.PUBCRATE) " (crate) " else ""} enum $constraintViolationSymbolName""",
        ) {
            renderConstraintViolations(writer)
        }

        renderImplDisplayConstraintViolation(writer)
        writer.rust("impl #T for ConstraintViolation { }", RuntimeType.StdError)

        if (shouldRenderAsValidationExceptionFieldList) {
            renderAsValidationExceptionFieldList(writer)
        }
    }

    /**
     * Returns the builder failure associated with the `member` field if its target is constrained.
     */
    fun builderConstraintViolationForMember(member: MemberShape) =
        if (builderTakesInUnconstrainedTypes && member.targetCanReachConstrainedShape(model, symbolProvider)) {
            ConstraintViolation(member, ConstraintViolationKind.CONSTRAINED_SHAPE_FAILURE)
        } else {
            null
        }

    /**
     * Returns the builder failure associated with the [member] field if it is `required`.
     */
    fun forMember(member: MemberShape): ConstraintViolation? {
        check(members.contains(member))
        // TODO(https://github.com/smithy-lang/smithy-rs/issues/1302, https://github.com/awslabs/smithy/issues/1179): See above.
        return if (symbolProvider.toSymbol(member).isOptional() || member.hasNonNullDefault()) {
            null
        } else {
            ConstraintViolation(member, ConstraintViolationKind.MISSING_MEMBER)
        }
    }

    // TODO(https://github.com/smithy-lang/smithy-rs/issues/1401) This impl does not take into account the `sensitive` trait.
    //   When constraint violation error messages are adjusted to match protocol tests, we should ensure it's honored.
    private fun renderImplDisplayConstraintViolation(writer: RustWriter) {
        writer.rustBlock("impl #T for ConstraintViolation", RuntimeType.Display) {
            rustBlock("fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result") {
                rustBlock("match self") {
                    all.forEach {
                        val arm =
                            if (it.hasInner()) {
                                "ConstraintViolation::${it.name()}(_)"
                            } else {
                                "ConstraintViolation::${it.name()}"
                            }
                        rust("""$arm => write!(f, "${it.message(symbolProvider, model)}"),""")
                    }
                }
            }
        }
    }

    private fun renderConstraintViolations(writer: RustWriter) {
        for (constraintViolation in all) {
            when (constraintViolation.kind) {
                ConstraintViolationKind.MISSING_MEMBER -> {
                    writer.docs(
                        "${
                            constraintViolation.message(symbolProvider, model).replaceFirstChar {
                                it.uppercaseChar()
                            }
                        }.",
                    )
                    writer.rust("${constraintViolation.name()},")
                }

                ConstraintViolationKind.CONSTRAINED_SHAPE_FAILURE -> {
                    val targetShape = model.expectShape(constraintViolation.forMember.target)

                    val constraintViolationSymbol =
                        constraintViolationSymbolProvider.toSymbol(targetShape)
                            // Box this constraint violation symbol if necessary.
                            .letIf(constraintViolation.forMember.hasTrait<ConstraintViolationRustBoxTrait>()) {
                                it.makeRustBoxed()
                            }

                    // Note we cannot express the inner constraint violation as `<T as TryFrom<T>>::Error`, because `T` might
                    // be `pub(crate)` and that would leak `T` in a public interface.
                    writer.docs(
                        "${constraintViolation.message(symbolProvider, model)}.".replaceFirstChar {
                            it.uppercaseChar()
                        },
                    )
                    Attribute.DocHidden.render(writer)
                    writer.rust("${constraintViolation.name()}(#T),", constraintViolationSymbol)
                }
            }
        }
    }

    private fun renderAsValidationExceptionFieldList(writer: RustWriter) {
        writer.rustTemplate(
            """
            impl ConstraintViolation {
                #{ValidationExceptionFnWritable:W}
            }
            """,
            "ValidationExceptionFnWritable" to validationExceptionConversionGenerator.builderConstraintViolationFn((all)),
            "String" to RuntimeType.String,
        )
    }
}

/**
 * The kinds of constraint violations that can occur when building the builder.
 */
enum class ConstraintViolationKind {
    // A field is required but was not provided.
    MISSING_MEMBER,

    // An unconstrained type was provided for a field targeting a constrained shape, but it failed to convert into the constrained type.
    CONSTRAINED_SHAPE_FAILURE,
}

data class ConstraintViolation(val forMember: MemberShape, val kind: ConstraintViolationKind) {
    fun name() =
        when (kind) {
            ConstraintViolationKind.MISSING_MEMBER -> "Missing${forMember.memberName.toPascalCase()}"
            ConstraintViolationKind.CONSTRAINED_SHAPE_FAILURE -> forMember.memberName.toPascalCase()
        }

    /**
     * Whether the constraint violation is a Rust tuple struct with one element.
     */
    fun hasInner() = kind == ConstraintViolationKind.CONSTRAINED_SHAPE_FAILURE

    /**
     * A message for a `ConstraintViolation` variant. This is used in both Rust documentation and the `Display` trait implementation.
     */
    fun message(
        symbolProvider: SymbolProvider,
        model: Model,
    ): String {
        val memberName = symbolProvider.toMemberName(forMember)
        val structureSymbol = symbolProvider.toSymbol(model.expectShape(forMember.container))
        return when (kind) {
            ConstraintViolationKind.MISSING_MEMBER -> "`$memberName` was not provided but it is required when building `${structureSymbol.name}`"
            // TODO(https://github.com/smithy-lang/smithy-rs/issues/1401) Nest errors. Adjust message following protocol tests.
            ConstraintViolationKind.CONSTRAINED_SHAPE_FAILURE -> "constraint violation occurred building member `$memberName` when building `${structureSymbol.name}`"
        }
    }
}
