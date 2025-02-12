/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.conditionalBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.std
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.makeMaybeConstrained
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.server.smithy.InlineModuleCreator
import software.amazon.smithy.rust.codegen.server.smithy.PubCrateConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.UnconstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.server.smithy.isDirectlyConstrained
import software.amazon.smithy.rust.codegen.server.smithy.traits.ConstraintViolationRustBoxTrait

/**
 * Generates a Rust type for a constrained collection shape that is able to hold values for the corresponding
 * _unconstrained_ shape. This type is a [RustType.Opaque] wrapper tuple newtype holding a `Vec`. Upon request parsing,
 * server deserializers use this type to store the incoming values without enforcing the modeled constraints. Only after
 * the full request has been parsed are constraints enforced, via the `impl TryFrom<UnconstrainedSymbol> for
 * ConstrainedSymbol`.
 *
 * This type is never exposed to the user; it is always `pub(crate)`. Only the deserializers use it.
 *
 * Consult [UnconstrainedShapeSymbolProvider] for more details and for an example.
 */
class UnconstrainedCollectionGenerator(
    val codegenContext: ServerCodegenContext,
    private val inlineModuleCreator: InlineModuleCreator,
    val shape: CollectionShape,
) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val unconstrainedShapeSymbolProvider = codegenContext.unconstrainedShapeSymbolProvider
    private val pubCrateConstrainedShapeSymbolProvider = codegenContext.pubCrateConstrainedShapeSymbolProvider
    private val symbol = unconstrainedShapeSymbolProvider.toSymbol(shape)
    private val name = symbol.name
    private val publicConstrainedTypes = codegenContext.settings.codegenConfig.publicConstrainedTypes
    private val constraintViolationSymbolProvider =
        with(codegenContext.constraintViolationSymbolProvider) {
            if (publicConstrainedTypes) {
                this
            } else {
                PubCrateConstraintViolationSymbolProvider(this)
            }
        }
    private val constraintViolationSymbol = constraintViolationSymbolProvider.toSymbol(shape)
    private val constrainedShapeSymbolProvider = codegenContext.constrainedShapeSymbolProvider
    private val constrainedSymbol =
        if (shape.isDirectlyConstrained(symbolProvider)) {
            constrainedShapeSymbolProvider.toSymbol(shape)
        } else {
            pubCrateConstrainedShapeSymbolProvider.toSymbol(shape)
        }
    private val innerShape = model.expectShape(shape.member.target)

    fun render() {
        check(shape.canReachConstrainedShape(model, symbolProvider))

        val innerMemberSymbol = unconstrainedShapeSymbolProvider.toSymbol(shape.member)

        inlineModuleCreator(symbol) {
            rustTemplate(
                """
                ##[derive(Debug, Clone)]
                pub(crate) struct $name(pub(crate) std::vec::Vec<#{InnerMemberSymbol}>);

                impl From<$name> for #{MaybeConstrained} {
                    fn from(value: $name) -> Self {
                        Self::Unconstrained(value)
                    }
                }
                """,
                "InnerMemberSymbol" to innerMemberSymbol,
                "MaybeConstrained" to constrainedSymbol.makeMaybeConstrained(),
            )

            renderTryFromUnconstrainedForConstrained(this)
        }
    }

    private fun renderTryFromUnconstrainedForConstrained(writer: RustWriter) {
        writer.rustBlock("impl std::convert::TryFrom<$name> for #{T}", constrainedSymbol) {
            rust("type Error = #T;", constraintViolationSymbol)

            rustBlock("fn try_from(value: $name) -> std::result::Result<Self, Self::Error>") {
                if (innerShape.canReachConstrainedShape(model, symbolProvider)) {
                    val resolvesToNonPublicConstrainedValueType =
                        innerShape.canReachConstrainedShape(model, symbolProvider) &&
                            !innerShape.isDirectlyConstrained(symbolProvider) &&
                            innerShape !is StructureShape &&
                            innerShape !is UnionShape
                    val constrainedMemberSymbol =
                        if (resolvesToNonPublicConstrainedValueType) {
                            pubCrateConstrainedShapeSymbolProvider.toSymbol(shape.member)
                        } else {
                            constrainedShapeSymbolProvider.toSymbol(shape.member)
                        }
                    val innerConstraintViolationSymbol = constraintViolationSymbolProvider.toSymbol(innerShape)
                    val boxErr =
                        if (shape.member.hasTrait<ConstraintViolationRustBoxTrait>()) {
                            ".map_err(|(idx, inner_violation)| (idx, Box::new(inner_violation)))"
                        } else {
                            ""
                        }
                    val constrainValueWritable =
                        writable {
                            conditionalBlock("inner.map(|inner| ", ").transpose()", constrainedMemberSymbol.isOptional()) {
                                rust("inner.try_into().map_err(|inner_violation| (idx, inner_violation))")
                            }
                        }

                    rustTemplate(
                        """
                        let res: #{Result}<#{Vec}<#{ConstrainedMemberSymbol}>, (usize, #{InnerConstraintViolationSymbol}) > = value
                            .0
                            .into_iter()
                            .enumerate()
                            .map(|(idx, inner)| {
                                #{ConstrainValueWritable:W}
                            })
                            .collect();
                        let inner = res
                            $boxErr
                            .map_err(|(idx, inner_violation)| Self::Error::Member(idx, inner_violation))?;
                        """,
                        "Vec" to RuntimeType.Vec,
                        "ConstrainedMemberSymbol" to constrainedMemberSymbol,
                        "InnerConstraintViolationSymbol" to innerConstraintViolationSymbol,
                        "ConstrainValueWritable" to constrainValueWritable,
                        "Result" to std.resolve("result::Result"),
                    )

                    val constrainedValueTypeIsNotFinalType =
                        resolvesToNonPublicConstrainedValueType && shape.isDirectlyConstrained(symbolProvider)
                    if (constrainedValueTypeIsNotFinalType) {
                        // Refer to the comments under `UnconstrainedMapGenerator` for a more in-depth explanation
                        // of this process. Consider the following Smithy model where a constrained list contains
                        // an indirectly constrained shape as a member:
                        //
                        // ```smithy
                        // @length(min: 1, max: 100)
                        // list ItemList {
                        //     member: Item
                        // }
                        //
                        // list Item {
                        //     member: ItemName
                        // }
                        //
                        // @length(min: 1, max: 100)
                        // string ItemName
                        // ```
                        //
                        // The final type exposed to the user is `ItemList<Vec<Vec<ItemName>>>`. However, the
                        // intermediate representation generated is `Vec<ItemConstrained>`. This needs to be
                        // transformed into `Vec<Vec<ItemName>>` to satisfy the `TryFrom` implementation and
                        // successfully construct an `ItemList` instance.
                        //
                        // This transformation is necessary due to the nested nature of the constraints and
                        // the difference between the internal constrained representation and the external
                        // user-facing type.
                        rustTemplate(
                            """
                            let inner: Vec<#{FinalType}> = inner.into_iter().map(|value| value.into()).collect();
                            """,
                            "FinalType" to symbolProvider.toSymbol(shape.member),
                        )
                    }
                } else {
                    rust("let inner = value.0;")
                }

                if (shape.isDirectlyConstrained(symbolProvider)) {
                    rust("Self::try_from(inner)")
                } else {
                    rust("Ok(Self(inner))")
                }
            }
        }
    }
}
