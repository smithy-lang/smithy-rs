/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.ByteShape
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.shapes.LongShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.ShortShape
import software.amazon.smithy.model.traits.RangeTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.documentShape
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.std
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.smithy.makeMaybeConstrained
import software.amazon.smithy.rust.codegen.core.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.redactIfNecessary
import software.amazon.smithy.rust.codegen.server.smithy.InlineModuleCreator
import software.amazon.smithy.rust.codegen.server.smithy.PubCrateConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.shapeConstraintViolationDisplayMessage
import software.amazon.smithy.rust.codegen.server.smithy.traits.isReachableFromOperationInput
import software.amazon.smithy.rust.codegen.server.smithy.validationErrorMessage

/**
 * [ConstrainedNumberGenerator] generates a wrapper newtype holding a constrained number primitive.
 * This type can be built from unconstrained values, yielding a `ConstraintViolation` when the input does not satisfy
 * the constraints.
 */
class ConstrainedNumberGenerator(
    val codegenContext: ServerCodegenContext,
    private val inlineModuleCreator: InlineModuleCreator,
    private val writer: RustWriter,
    val shape: NumberShape,
    private val validationExceptionConversionGenerator: ValidationExceptionConversionGenerator,
) {
    val model = codegenContext.model
    val constrainedShapeSymbolProvider = codegenContext.constrainedShapeSymbolProvider
    val publicConstrainedTypes = codegenContext.settings.codegenConfig.publicConstrainedTypes

    private val unconstrainedType =
        when (shape) {
            is ByteShape -> RustType.Integer(8)
            is ShortShape -> RustType.Integer(16)
            is IntegerShape -> RustType.Integer(32)
            is LongShape -> RustType.Integer(64)
            else -> UNREACHABLE("Trying to generate a constrained number for an unsupported Smithy number shape")
        }

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
        val name = symbol.name
        val unconstrainedTypeName = unconstrainedType.render()
        val constraintViolation = constraintViolationSymbolProvider.toSymbol(shape)
        val rangeInfo = Range(rangeTrait)
        val constraintsInfo = listOf(rangeInfo.toTraitInfo())

        writer.documentShape(shape, model)
        writer.docs(rustDocsConstrainedTypeEpilogue(name))
        val metadata = symbol.expectRustMetadata()
        metadata.render(writer)
        writer.rust("struct $name(pub(crate) $unconstrainedTypeName);")

        if (metadata.visibility == Visibility.PUBCRATE) {
            Attribute.AllowDeadCode.render(writer)
        }
        writer.rustTemplate(
            """
            impl $name {
                /// ${rustDocsInnerMethod(unconstrainedTypeName)}
                pub fn inner(&self) -> &$unconstrainedTypeName {
                    &self.0
                }

                /// ${rustDocsIntoInnerMethod(unconstrainedTypeName)}
                pub fn into_inner(self) -> $unconstrainedTypeName {
                    self.0
                }
            }

            impl #{ConstrainedTrait} for $name  {
                type Unconstrained = $unconstrainedTypeName;
            }

            impl #{From}<$unconstrainedTypeName> for #{MaybeConstrained} {
                fn from(value: $unconstrainedTypeName) -> Self {
                    Self::Unconstrained(value)
                }
            }

            impl #{Display} for $name {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                   ${shape.redactIfNecessary(model, "self.0")}.fmt(f)
                }
            }

            impl #{From}<$name> for $unconstrainedTypeName {
                fn from(value: $name) -> Self {
                    value.into_inner()
                }
            }
            """,
            "ConstrainedTrait" to RuntimeType.ConstrainedTrait,
            "ConstraintViolation" to constraintViolation,
            "MaybeConstrained" to symbol.makeMaybeConstrained(),
            "Display" to RuntimeType.Display,
            "From" to RuntimeType.From,
            "TryFrom" to RuntimeType.TryFrom,
            "AsRef" to RuntimeType.AsRef,
        )

        writer.renderTryFrom(unconstrainedTypeName, name, constraintViolation, constraintsInfo)

        inlineModuleCreator(constraintViolation) {
            rustTemplate(
                """
                ##[derive(Debug, PartialEq)]
                pub enum ${constraintViolation.name} {
                    Range($unconstrainedTypeName),
                }

                impl #{Display} for ${constraintViolation.name} {
                    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(f, "${rangeInfo.rangeTrait.shapeConstraintViolationDisplayMessage(shape).replace("#", "##")}")
                    }
                }
                    
                impl #{Error} for ${constraintViolation.name} {}
                """,
                "Error" to RuntimeType.StdError,
                "Display" to RuntimeType.Display,
            )

            if (shape.isReachableFromOperationInput()) {
                rustTemplate(
                    """
                    impl ${constraintViolation.name} {
                        #{NumberShapeConstraintViolationImplBlock}
                    }
                    """,
                    "NumberShapeConstraintViolationImplBlock" to validationExceptionConversionGenerator.numberShapeConstraintViolationImplBlock(rangeInfo),
                )
            }
        }
    }
}

data class Range(val rangeTrait: RangeTrait) {
    fun toTraitInfo(): TraitInfo =
        TraitInfo(
            { rust("Self::check_range(value)?;") },
            { docs("Error when a number doesn't satisfy its `@range` requirements.") },
            {
                rust(
                    """
                    Self::Range(_) => crate::model::ValidationExceptionField {
                        message: format!("${rangeTrait.validationErrorMessage()}", &path),
                        path,
                    },
                    """,
                )
            },
            this::renderValidationFunction,
        )

    /**
     * Renders a `check_range` function to validate that the value matches the
     * required range indicated by the `@range` trait.
     */
    private fun renderValidationFunction(
        constraintViolation: Symbol,
        unconstrainedTypeName: String,
    ): Writable =
        {
            val valueVariableName = "value"
            val condition =
                if (rangeTrait.min.isPresent && rangeTrait.max.isPresent) {
                    "(${rangeTrait.min.get()}..=${rangeTrait.max.get()}).contains(&$valueVariableName)"
                } else if (rangeTrait.min.isPresent) {
                    "${rangeTrait.min.get()} <= $valueVariableName"
                } else {
                    "$valueVariableName <= ${rangeTrait.max.get()}"
                }

            rustTemplate(
                """
                fn check_range($valueVariableName: $unconstrainedTypeName) -> #{Result}<(), $constraintViolation> {
                    if $condition {
                        Ok(())
                    } else {
                        Err($constraintViolation::Range($valueVariableName))
                    }
                }
                """,
                "Result" to std.resolve("result::Result"),
            )
        }
}
