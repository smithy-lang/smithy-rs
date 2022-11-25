/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.traits.RangeTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.documentShape
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.makeMaybeConstrained
import software.amazon.smithy.rust.codegen.core.smithy.module
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.redactIfNecessary
import software.amazon.smithy.rust.codegen.server.smithy.PubCrateConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.traits.isReachableFromOperationInput
import software.amazon.smithy.rust.codegen.server.smithy.validationErrorMessage

/**
 * [ConstrainedIntegerGenerator] generates a wrapper newtype holding a constrained `i32`.
 * This type can be built from unconstrained values, yielding a `ConstraintViolation` when the input does not satisfy
 * the constraints.
 */
class ConstrainedIntegerGenerator(
    val codegenContext: ServerCodegenContext,
    val writer: RustWriter,
    val shape: IntegerShape,
) {
    val model = codegenContext.model
    val constrainedShapeSymbolProvider = codegenContext.constrainedShapeSymbolProvider
    val publicConstrainedTypes = codegenContext.settings.codegenConfig.publicConstrainedTypes
    private val constraintViolationSymbolProvider =
        with(codegenContext.constraintViolationSymbolProvider) {
            if (publicConstrainedTypes) {
                this
            } else {
                PubCrateConstraintViolationSymbolProvider(this)
            }
        }

    fun render() {
        val rangeTrait = shape.expectTrait<RangeTrait>()

        val symbol = constrainedShapeSymbolProvider.toSymbol(shape)
        val constrainedTypeName = symbol.name
        val unconstrainedTypeName = RustType.Integer(32).render()
        val constraintViolation = constraintViolationSymbolProvider.toSymbol(shape)
        val constraintsInfo = listOf(Range(rangeTrait).toTraitInfo(unconstrainedTypeName))

        val constrainedTypeVisibility = if (publicConstrainedTypes) {
            Visibility.PUBLIC
        } else {
            Visibility.PUBCRATE
        }
        val constrainedTypeMetadata = RustMetadata(
            Attribute.Derives(
                setOf(
                    RuntimeType.Debug,
                    RuntimeType.Clone,
                    RuntimeType.PartialEq,
                    RuntimeType.Eq,
                    RuntimeType.Hash,
                ),
            ),
            visibility = constrainedTypeVisibility,
        )

        writer.documentShape(shape, model, note = rustDocsNote(constrainedTypeName))
        constrainedTypeMetadata.render(writer)
        writer.rust("struct $constrainedTypeName(pub(crate) $unconstrainedTypeName);")
        if (constrainedTypeVisibility == Visibility.PUBCRATE) {
            Attribute.AllowUnused.render(writer)
        }
        writer.renderTryFrom(unconstrainedTypeName, constrainedTypeName, constraintViolation, constraintsInfo)
        writer.rustTemplate(
            """
            impl $constrainedTypeName {
                /// ${rustDocsInnerMethod(unconstrainedTypeName)}
                pub fn inner(&self) -> &$unconstrainedTypeName {
                    &self.0
                }

                /// ${rustDocsIntoInnerMethod(unconstrainedTypeName)}
                pub fn into_inner(self) -> $unconstrainedTypeName {
                    self.0
                }
            }

            impl #{ConstrainedTrait} for $constrainedTypeName  {
                type Unconstrained = $unconstrainedTypeName;
            }

            impl #{From}<$unconstrainedTypeName> for #{MaybeConstrained} {
                fn from(value: $unconstrainedTypeName) -> Self {
                    Self::Unconstrained(value)
                }
            }

            impl #{Display} for $constrainedTypeName {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                   ${shape.redactIfNecessary(model, "self.0")}.fmt(f)
                }
            }

            impl #{From}<$constrainedTypeName> for $unconstrainedTypeName {
                fn from(value: $constrainedTypeName) -> Self {
                    value.into_inner()
                }
            }
            """,
            "ConstrainedTrait" to RuntimeType.ConstrainedTrait(),
            "ConstraintViolation" to constraintViolation,
            "MaybeConstrained" to symbol.makeMaybeConstrained(),
            "Display" to RuntimeType.Display,
            "From" to RuntimeType.From,
            "TryFrom" to RuntimeType.TryFrom,
            "AsRef" to RuntimeType.AsRef,
        )

        writer.withInlineModule(constraintViolation.module()) {
            rust(
                """
                ##[derive(Debug, PartialEq)]
                pub enum ${constraintViolation.name} {
                    Range($unconstrainedTypeName),
                }
                """,
            )

            if (shape.isReachableFromOperationInput()) {
                rustBlock("impl ${constraintViolation.name}") {
                    rustBlockTemplate(
                        "pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::ValidationExceptionField",
                        "String" to RuntimeType.String,
                    ) {
                        rustBlock("match self") {
                            rust(
                                """
                                Self::Range(value) => crate::model::ValidationExceptionField {
                                    message: format!("${rangeTrait.validationErrorMessage()}", value, &path),
                                    path,
                                },
                                """,
                            )
                        }
                    }
                }
            }
        }
    }
}

private data class Range(val rangeTrait: RangeTrait) {
    fun toTraitInfo(unconstrainedTypeName: String): TraitInfo = TraitInfo(
        { rust("Self::check_range(value)?;") },
        {
            docs("Error when an integer doesn't satisfy its `@range` requirements.")
            rust("Range($unconstrainedTypeName)")
        },
        {
            rust(
                """
                Self::Range(value) => crate::model::ValidationExceptionField {
                    message: format!("${rangeTrait.validationErrorMessage()}", value, &path),
                    path,
                },
                """,
            )
        },
        this::renderValidationFunction,
    )

    /**
     * Renders a `check_range` function to validate the integer matches the
     * required range indicated by the `@range` trait.
     */
    private fun renderValidationFunction(constraintViolation: Symbol, unconstrainedTypeName: String): Writable = {
        val valueVariableName = "value"
        val condition = if (rangeTrait.min.isPresent && rangeTrait.max.isPresent) {
            "(${rangeTrait.min.get()}..=${rangeTrait.max.get()}).contains(&$valueVariableName)"
        } else if (rangeTrait.min.isPresent) {
            "${rangeTrait.min.get()} <= $valueVariableName"
        } else {
            "$valueVariableName <= ${rangeTrait.max.get()}"
        }

        rust(
            """
            fn check_range($valueVariableName: $unconstrainedTypeName) -> Result<(), $constraintViolation> {
                if $condition {
                    Ok(())
                } else {
                    Err($constraintViolation::Range($valueVariableName))
                }
            }
            """,
        )
    }
}
