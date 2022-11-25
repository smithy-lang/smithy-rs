/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.server.smithy.PubCrateConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.server.smithy.traits.isReachableFromOperationInput
import software.amazon.smithy.rust.codegen.server.smithy.validationErrorMessage

class CollectionConstraintViolationGenerator(
    codegenContext: ServerCodegenContext,
    private val modelsModuleWriter: RustWriter,
    val shape: CollectionShape,
//    val constraintsInfo : List<TraitInfo>,
) {
    private val model = codegenContext.model
    private val constrainedShapeSymbolProvider = codegenContext.constrainedShapeSymbolProvider
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

        val constraintViolationCodegenScopeMutableList: MutableList<Pair<String, Any>> = mutableListOf()
        val isMemberConstrained = memberShape.canReachConstrainedShape(model, symbolProvider)

        if (isMemberConstrained) {
            constraintViolationCodegenScopeMutableList.add("MemberConstraintViolationSymbol" to constraintViolationSymbolProvider.toSymbol(memberShape))
        }

        val constraintViolationCodegenScope = constraintViolationCodegenScopeMutableList.toTypedArray()

        val constraintViolationVisibility = Visibility.publicIf(publicConstrainedTypes, Visibility.PUBCRATE)

        modelsModuleWriter.withModule(
            RustModule(
                constraintViolationSymbol.namespace.split(constraintViolationSymbol.namespaceDelimiter).last(),
                RustMetadata(visibility = constraintViolationVisibility),
            ),
        ) {
            // TODO(https://github.com/awslabs/smithy-rs/issues/1401) We should really have two `ConstraintViolation`
            //  types here. One will just have variants for each constraint trait on the collection shape, for use by the user.
            //  The other one will have variants if the shape's member is directly or transitively constrained,
            //  and is for use by the framework.
            rustTemplate(
                """
                ##[derive(Debug, PartialEq)]
                ${constraintViolationVisibility.toRustQualifier()} enum $constraintViolationName {
                    ${if (shape.hasTrait<LengthTrait>()) "Length(usize)," else ""}
                    ${if (isMemberConstrained) "##[doc(hidden)] Member(usize, #{MemberConstraintViolationSymbol})," else ""}
                }
                """,
                *constraintViolationCodegenScope,
            )

            if (shape.isReachableFromOperationInput()) {
                rustBlock("impl $constraintViolationName") {
                    rustBlockTemplate(
                        "pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::ValidationExceptionField",
                        "String" to RuntimeType.String,
                    ) {
                        rustBlock("match self") {
                            shape.getTrait<LengthTrait>()?.also {
                                rust(
                                    """
                                    Self::Length(length) => crate::model::ValidationExceptionField {
                                        message: format!("${it.validationErrorMessage()}", length, &path),
                                        path,
                                    },
                                    """,
                                )
                            }
                            if (isMemberConstrained) {
                                rust("""Self::Member(index, member_constraint_violation) => member_constraint_violation.as_validation_exception_field(path + "/" + &index.to_string()),""")
                            }
                        }
                    }
                }
            }
        }
    }
}
