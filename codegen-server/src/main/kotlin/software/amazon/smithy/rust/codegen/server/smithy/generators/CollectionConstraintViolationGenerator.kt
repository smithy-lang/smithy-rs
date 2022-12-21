/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.module
import software.amazon.smithy.rust.codegen.server.smithy.PubCrateConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.server.smithy.traits.isReachableFromOperationInput

class CollectionConstraintViolationGenerator(
    codegenContext: ServerCodegenContext,
    private val modelsModuleWriter: RustWriter,
    private val shape: CollectionShape,
    private val constraintsInfo: List<TraitInfo>,
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

    fun render() {
        val memberShape = model.expectShape(shape.member.target)
        val constraintViolationSymbol = constraintViolationSymbolProvider.toSymbol(shape)
        val constraintViolationName = constraintViolationSymbol.name
        val isMemberConstrained = memberShape.canReachConstrainedShape(model, symbolProvider)
        val constraintViolationVisibility = Visibility.publicIf(publicConstrainedTypes, Visibility.PUBCRATE)

        modelsModuleWriter.withInlineModule(constraintViolationSymbol.module()) {
            val constraintViolationVariants = constraintsInfo.map { it.constraintViolationVariant }.toMutableList()
            if (isMemberConstrained) {
                constraintViolationVariants += {
                    rustTemplate(
                        """
                        /// Constraint violation error when an element doesn't satisfy its own constraints.
                        /// The first component of the tuple is the index in the collection where the
                        /// first constraint violation was found.
                        ##[doc(hidden)]
                        Member(usize, #{MemberConstraintViolationSymbol})
                        """,
                        "MemberConstraintViolationSymbol" to constraintViolationSymbolProvider.toSymbol(memberShape),
                    )
                }
            }

            // TODO(https://github.com/awslabs/smithy-rs/issues/1401) We should really have two `ConstraintViolation`
            //  types here. One will just have variants for each constraint trait on the collection shape, for use by the user.
            //  The other one will have variants if the shape's member is directly or transitively constrained,
            //  and is for use by the framework.
            rustTemplate(
                """
                ##[derive(Debug, PartialEq)]
                ${constraintViolationVisibility.toRustQualifier()} enum $constraintViolationName {
                    #{ConstraintViolationVariants:W}
                }
                """,
                "ConstraintViolationVariants" to constraintViolationVariants.join(",\n"),
            )

            if (shape.isReachableFromOperationInput()) {
                val validationExceptionFields = constraintsInfo.map { it.asValidationExceptionField }.toMutableList()
                if (isMemberConstrained) {
                    validationExceptionFields += {
                        rust(
                            """
                            Self::Member(index, member_constraint_violation) =>
                                member_constraint_violation.as_validation_exception_field(path + "/" + &index.to_string())
                            """,
                        )
                    }
                }

                rustTemplate(
                    """
                    impl $constraintViolationName {
                        pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::ValidationExceptionField {
                            match self {
                                #{AsValidationExceptionFields:W}
                            }
                        }
                    }
                    """,
                    "String" to RuntimeType.String,
                    "AsValidationExceptionFields" to validationExceptionFields.join("\n"),
                )
            }
        }
    }
}
