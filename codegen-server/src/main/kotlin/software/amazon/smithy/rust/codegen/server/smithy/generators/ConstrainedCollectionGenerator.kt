/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.model.traits.Trait
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.documentShape
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.server.smithy.PubCrateConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.server.smithy.supportedCollectionConstraintTraits
import software.amazon.smithy.rust.codegen.server.smithy.validationErrorMessage

/**
 * [ConstrainedCollectionGenerator] generates a wrapper tuple newtype holding a constrained `std::vec::Vec`.
 * This type can be built from unconstrained values, yielding a `ConstraintViolation` when the input does not satisfy
 * the constraints.
 *
 * The [`length`] and [`uniqueItems`] traits are the only constraint traits applicable to list shapes.
 * TODO(https://github.com/awslabs/smithy-rs/issues/1401):
 * The [`uniqueItems`] trait has not been implemented yet.
 *
 * If [unconstrainedSymbol] is provided, the `MaybeConstrained` trait is implemented for the constrained type, using the
 * [unconstrainedSymbol]'s associated type as the associated type for the trait.
 *
 * [`length`]: https://smithy.io/2.0/spec/constraint-traits.html#length-trait
 * [`uniqueItems`]: https://smithy.io/2.0/spec/constraint-traits.html#smithy-api-uniqueitems-trait
 */
class ConstrainedCollectionGenerator(
    val codegenContext: ServerCodegenContext,
    val writer: RustWriter,
    val shape: CollectionShape,
    private val constraintsInfo: List<TraitInfo>,
    private val unconstrainedSymbol: Symbol? = null,
) {
    private val model = codegenContext.model
    private val constrainedShapeSymbolProvider = codegenContext.constrainedShapeSymbolProvider
    private val publicConstrainedTypes = codegenContext.settings.codegenConfig.publicConstrainedTypes
    private val constraintViolationSymbolProvider =
        with(codegenContext.constraintViolationSymbolProvider) {
            if (publicConstrainedTypes) {
                this
            } else {
                PubCrateConstraintViolationSymbolProvider(this)
            }
        }
    private val symbolProvider = codegenContext.symbolProvider

    fun render() {
        check(constraintsInfo.isNotEmpty()) {
            "`ConstrainedCollectionGenerator` can only be invoked for constrained collections, but this shape was not constrained"
        }

        val name = constrainedShapeSymbolProvider.toSymbol(shape).name
        val inner = "std::vec::Vec<#{ValueSymbol}>"
        val constraintViolation = constraintViolationSymbolProvider.toSymbol(shape)
        val constrainedTypeVisibility = Visibility.publicIf(publicConstrainedTypes, Visibility.PUBCRATE)
        val constrainedTypeMetadata = RustMetadata(
            Attribute.Derives(setOf(RuntimeType.Debug, RuntimeType.Clone, RuntimeType.PartialEq)),
            visibility = constrainedTypeVisibility,
        )

        val codegenScope = arrayOf(
            "ValueSymbol" to constrainedShapeSymbolProvider.toSymbol(model.expectShape(shape.member.target)),
            "From" to RuntimeType.From,
            "TryFrom" to RuntimeType.TryFrom,
            "ConstraintViolation" to constraintViolation,
        )

        writer.documentShape(shape, model, note = rustDocsNote(name))
        constrainedTypeMetadata.render(writer)
        writer.rustTemplate(
            """
            struct $name(pub(crate) $inner);
            """,
            *codegenScope,
        )
        if (constrainedTypeVisibility == Visibility.PUBCRATE) {
            Attribute.AllowUnused.render(writer)
        }

        writer.rustTemplate(
            """
            impl $name {
                /// ${rustDocsInnerMethod(inner)}
                pub fn inner(&self) -> &$inner {
                    &self.0
                }

                /// ${rustDocsIntoInnerMethod(inner)}
                pub fn into_inner(self) -> $inner {
                    self.0
                }

                #{ValidationFunctions:W}
            }

            impl #{TryFrom}<$inner> for $name {
                type Error = #{ConstraintViolation};

                /// ${rustDocsTryFromMethod(name, inner)}
                fn try_from(value: $inner) -> Result<Self, Self::Error> {
                    #{ConstraintChecks:W}

                    Ok(Self(value))
                }
            }

            impl #{From}<$name> for $inner {
                fn from(value: $name) -> Self {
                    value.into_inner()
                }
            }
            """,
            *codegenScope,
            "ConstraintChecks" to constraintsInfo.map { it.tryFromCheck }.join("\n"),
            "ValidationFunctions" to constraintsInfo.map { it.validationFunctionDefinition(constraintViolation, inner) }.join("\n"),
        )

        val innerShape = model.expectShape(shape.member.target)
        if (!publicConstrainedTypes &&
            innerShape.canReachConstrainedShape(model, symbolProvider) &&
            innerShape !is StructureShape &&
            innerShape !is UnionShape
        ) {
            writer.rustTemplate(
                """
                impl #{From}<$name> for #{FullyUnconstrainedSymbol} {
                    fn from(value: $name) -> Self {
                        value
                            .into_inner()
                            .into_iter()
                            .map(|v| v.into())
                            .collect()
                    }
                }
                """,
                *codegenScope,
                "FullyUnconstrainedSymbol" to symbolProvider.toSymbol(shape),
            )
        }

        if (unconstrainedSymbol != null) {
            writer.rustTemplate(
                """
                impl #{ConstrainedTrait} for $name {
                    type Unconstrained = #{UnconstrainedSymbol};
                }
                """,
                "ConstrainedTrait" to RuntimeType.ConstrainedTrait,
                "UnconstrainedSymbol" to unconstrainedSymbol,
            )
        }
    }
}

internal sealed class CollectionTraitInfo {
    data class Length(val lengthTrait: LengthTrait) : CollectionTraitInfo() {
        override fun toTraitInfo(): TraitInfo =
            TraitInfo(
                tryFromCheck = {
                    rust("Self::check_length(value.len())?;")
                },
                constraintViolationVariant = {
                    docs("Constraint violation error when the vector doesn't have the required length")
                    rust("Length(usize)")
                },
                asValidationExceptionField = {
                    rust(
                        """
                        Self::Length(length) => crate::model::ValidationExceptionField {
                            message: format!("${lengthTrait.validationErrorMessage()}", length, &path),
                            path,
                        },
                        """,
                    )
                },
                validationFunctionDefinition = { constraintViolation, _ ->
                    {
                        rustTemplate(
                            """
                            fn check_length(length: usize) -> Result<(), #{ConstraintViolation}> {
                                if ${lengthTrait.rustCondition("length")} {
                                    Ok(())
                                } else {
                                    Err(#{ConstraintViolation}::Length(length))
                                }
                            }
                            """,
                            "ConstraintViolation" to constraintViolation,
                        )
                    }
                },
            )
    }

    companion object {
        private fun fromTrait(trait: Trait): CollectionTraitInfo =
            when (trait) {
                is LengthTrait -> {
                    Length(trait)
                }
                // TODO(https://github.com/awslabs/smithy-rs/issues/1670): Not implemented yet.
                // is UniqueItemsTrait -> {
                //     UniqueItems(trait)
                // }
                else -> {
                    PANIC("CollectionTraitInfo.fromTrait called with unsupported trait $trait")
                }
            }

        fun fromShape(shape: CollectionShape): List<TraitInfo> =
            supportedCollectionConstraintTraits
                .mapNotNull { shape.getTrait(it).orNull() }
                .map(CollectionTraitInfo::fromTrait)
                .map(CollectionTraitInfo::toTraitInfo)
    }

    abstract fun toTraitInfo(): TraitInfo
}
