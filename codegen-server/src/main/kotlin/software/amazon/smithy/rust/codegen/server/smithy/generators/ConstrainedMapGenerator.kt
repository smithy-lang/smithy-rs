/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.documentShape
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.server.smithy.PubCrateConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.typeNameContainsNonPublicType

/**
 * [ConstrainedMapGenerator] generates a wrapper tuple newtype holding a constrained `std::collections::HashMap`.
 * This type can be built from unconstrained values, yielding a `ConstraintViolation` when the input does not satisfy
 * the constraints.
 *
 * The [`length` trait] is the only constraint trait applicable to map shapes.
 *
 * If [unconstrainedSymbol] is provided, the `MaybeConstrained` trait is implemented for the constrained type, using the
 * [unconstrainedSymbol]'s associated type as the associated type for the trait.
 *
 * [`length` trait]: https://smithy.io/2.0/spec/constraint-traits.html#length-trait
 */
class ConstrainedMapGenerator(
    val codegenContext: ServerCodegenContext,
    val writer: RustWriter,
    val shape: MapShape,
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
        // The `length` trait is the only constraint trait applicable to map shapes.
        val lengthTrait = shape.expectTrait<LengthTrait>()

        val name = constrainedShapeSymbolProvider.toSymbol(shape).name
        val inner = "#{HashMap}<#{KeySymbol}, #{ValueMemberSymbol}>"
        val constraintViolation = constraintViolationSymbolProvider.toSymbol(shape)
        val constrainedSymbol = symbolProvider.toSymbol(shape)

        val codegenScope =
            arrayOf(
                "HashMap" to RuntimeType.HashMap,
                "KeySymbol" to constrainedShapeSymbolProvider.toSymbol(model.expectShape(shape.key.target)),
                "ValueMemberSymbol" to constrainedShapeSymbolProvider.toSymbol(shape.value),
                "From" to RuntimeType.From,
                "TryFrom" to RuntimeType.TryFrom,
                "ConstraintViolation" to constraintViolation,
                *RuntimeType.preludeScope,
            )

        writer.documentShape(shape, model)
        writer.docs(rustDocsConstrainedTypeEpilogue(name))
        val metadata = constrainedSymbol.expectRustMetadata()
        metadata.render(writer)
        writer.rustTemplate("struct $name(pub(crate) $inner);", *codegenScope)
        writer.rustBlockTemplate("impl $name", *codegenScope) {
            if (metadata.visibility == Visibility.PUBLIC) {
                writer.rustTemplate(
                    """
                    /// ${rustDocsInnerMethod(inner)}
                    pub fn inner(&self) -> &$inner {
                        &self.0
                    }
                    """,
                    *codegenScope,
                )
            }
            writer.rustTemplate(
                """
                /// ${rustDocsIntoInnerMethod(inner)}
                pub fn into_inner(self) -> $inner {
                    self.0
                }
                """,
                *codegenScope,
            )
        }

        writer.rustTemplate(
            """
            impl #{TryFrom}<$inner> for $name {
                type Error = #{ConstraintViolation};

                /// ${rustDocsTryFromMethod(name, inner)}
                fn try_from(value: $inner) -> #{Result}<Self, Self::Error> {
                    let length = value.len();
                    if ${lengthTrait.rustCondition("length")} {
                        Ok(Self(value))
                    } else {
                        Err(#{ConstraintViolation}::Length(length))
                    }
                }
            }

            impl #{From}<$name> for $inner {
                fn from(value: $name) -> Self {
                    value.into_inner()
                }
            }
            """,
            *codegenScope,
        )

        val valueShape = model.expectShape(shape.value.target)
        if (!publicConstrainedTypes &&
            isValueConstrained(valueShape, model, symbolProvider) &&
            valueShape !is StructureShape &&
            valueShape !is UnionShape
        ) {
            val keyShape = model.expectShape(shape.key.target, StringShape::class.java)
            val keyNeedsConversion = keyShape.typeNameContainsNonPublicType(model, symbolProvider, publicConstrainedTypes)
            val key =
                if (keyNeedsConversion) {
                    "k.into()"
                } else {
                    "k"
                }

            writer.rustTemplate(
                """
                impl #{From}<$name> for #{FullyUnconstrainedSymbol} {
                    fn from(value: $name) -> Self {
                        value
                            .into_inner()
                            .into_iter()
                            .map(|(k, v)| ($key, v.into()))
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
                impl #{ConstrainedTrait} for $name  {
                    type Unconstrained = #{UnconstrainedSymbol};
                }
                """,
                *codegenScope,
                "ConstrainedTrait" to RuntimeType.ConstrainedTrait,
                "UnconstrainedSymbol" to unconstrainedSymbol,
            )
        }
    }
}
