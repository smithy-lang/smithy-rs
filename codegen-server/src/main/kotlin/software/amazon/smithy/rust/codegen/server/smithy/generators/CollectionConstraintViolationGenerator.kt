/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.makeRustBoxed
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.server.smithy.InlineModuleCreator
import software.amazon.smithy.rust.codegen.server.smithy.PubCrateConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.server.smithy.traits.ConstraintViolationRustBoxTrait
import software.amazon.smithy.rust.codegen.server.smithy.traits.isReachableFromOperationInput

class CollectionConstraintViolationGenerator(
    codegenContext: ServerCodegenContext,
    private val inlineModuleCreator: InlineModuleCreator,
    private val shape: CollectionShape,
    private val collectionConstraintsInfo: List<CollectionTraitInfo>,
    private val validationExceptionConversionGenerator: ValidationExceptionConversionGenerator,
) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val publicConstrainedTypes = codegenContext.settings.codegenConfig.publicConstrainedTypes
    private val constraintViolationSymbolProvider =
        with(codegenContext.constraintViolationSymbolProvider) {
            if (publicConstrainedTypes) {
                this
            } else {
                PubCrateConstraintViolationSymbolProvider(this)
            }
        }
    private val constraintsInfo: List<TraitInfo> = collectionConstraintsInfo.map { it.toTraitInfo() }

    fun render() {
        val targetShape = model.expectShape(shape.member.target)
        val constraintViolationSymbol = constraintViolationSymbolProvider.toSymbol(shape)
        val constraintViolationName = constraintViolationSymbol.name
        val isMemberConstrained = targetShape.canReachConstrainedShape(model, symbolProvider)
        val constraintViolationVisibility = Visibility.publicIf(publicConstrainedTypes, Visibility.PUBCRATE)

        inlineModuleCreator(constraintViolationSymbol) {
            val constraintViolationVariants = constraintsInfo.map { it.constraintViolationVariant }.toMutableList()
            if (shape.isReachableFromOperationInput() && isMemberConstrained) {
                constraintViolationVariants += {
                    val memberConstraintViolationSymbol =
                        constraintViolationSymbolProvider.toSymbol(targetShape).letIf(
                            shape.member.hasTrait<ConstraintViolationRustBoxTrait>(),
                        ) {
                            it.makeRustBoxed()
                        }
                    rustTemplate(
                        """
                        /// Constraint violation error when an element doesn't satisfy its own constraints.
                        /// The first component of the tuple is the index in the collection where the
                        /// first constraint violation was found.
                        ##[doc(hidden)]
                        Member(usize, #{MemberConstraintViolationSymbol})
                        """,
                        "MemberConstraintViolationSymbol" to memberConstraintViolationSymbol,
                    )
                }
            }

            // TODO(https://github.com/awslabs/smithy-rs/issues/1401) We should really have two `ConstraintViolation`
            //  types here. One will just have variants for each constraint trait on the collection shape, for use by the user.
            //  The other one will have variants if the shape's member is directly or transitively constrained,
            //  and is for use by the framework.
            rustTemplate(
                """
                ##[allow(clippy::enum_variant_names)]
                ##[derive(Debug, PartialEq)]
                ${constraintViolationVisibility.toRustQualifier()} enum $constraintViolationName {
                    #{ConstraintViolationVariants:W}
                }
                """,
                "ConstraintViolationVariants" to constraintViolationVariants.join(",\n"),
            )

            if (shape.isReachableFromOperationInput()) {
                rustTemplate(
                    """
                    impl $constraintViolationName {
                        #{CollectionShapeConstraintViolationImplBlock}
                    }
                    """,
                    "CollectionShapeConstraintViolationImplBlock" to validationExceptionConversionGenerator.collectionShapeConstraintViolationImplBlock(collectionConstraintsInfo, isMemberConstrained),
                )
            }
        }
    }
}
