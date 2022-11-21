/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.module
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.server.smithy.PubCrateConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.traits.isReachableFromOperationInput
import software.amazon.smithy.rust.codegen.server.smithy.validationErrorMessage

class MapConstraintViolationGenerator(
    codegenContext: ServerCodegenContext,
    private val modelsModuleWriter: RustWriter,
    val shape: MapShape,
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
        val keyShape = model.expectShape(shape.key.target, StringShape::class.java)
        val valueShape = model.expectShape(shape.value.target)
        val constraintViolationSymbol = constraintViolationSymbolProvider.toSymbol(shape)
        val constraintViolationName = constraintViolationSymbol.name

        val constraintViolationCodegenScopeMutableList: MutableList<Pair<String, Any>> = mutableListOf()
        if (isKeyConstrained(keyShape, symbolProvider)) {
            constraintViolationCodegenScopeMutableList.add("KeyConstraintViolationSymbol" to constraintViolationSymbolProvider.toSymbol(keyShape))
        }
        if (isValueConstrained(valueShape, model, symbolProvider)) {
            constraintViolationCodegenScopeMutableList.add("ValueConstraintViolationSymbol" to constraintViolationSymbolProvider.toSymbol(valueShape))
            constraintViolationCodegenScopeMutableList.add("KeySymbol" to constrainedShapeSymbolProvider.toSymbol(keyShape))
        }
        val constraintViolationCodegenScope = constraintViolationCodegenScopeMutableList.toTypedArray()

        val constraintViolationVisibility = if (publicConstrainedTypes) {
            Visibility.PUBLIC
        } else {
            Visibility.PUBCRATE
        }
        modelsModuleWriter.withInlineModule(constraintViolationSymbol.module()) {
            // TODO(https://github.com/awslabs/smithy-rs/issues/1401) We should really have two `ConstraintViolation`
            //  types here. One will just have variants for each constraint trait on the map shape, for use by the user.
            //  The other one will have variants if the shape's key or value is directly or transitively constrained,
            //  and is for use by the framework.
            rustTemplate(
                """
                ##[derive(Debug, PartialEq)]
                pub${ if (constraintViolationVisibility == Visibility.PUBCRATE) " (crate) " else "" } enum $constraintViolationName {
                    ${if (shape.hasTrait<LengthTrait>()) "Length(usize)," else ""}
                    ${if (isKeyConstrained(keyShape, symbolProvider)) "##[doc(hidden)] Key(#{KeyConstraintViolationSymbol})," else ""}
                    ${if (isValueConstrained(valueShape, model, symbolProvider)) "##[doc(hidden)] Value(#{KeySymbol}, #{ValueConstraintViolationSymbol})," else ""}
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
                            if (isKeyConstrained(keyShape, symbolProvider)) {
                                // Note how we _do not_ append the key's member name to the path. This is intentional, as
                                // per the `RestJsonMalformedLengthMapKey` test. Note keys are always strings.
                                // https://github.com/awslabs/smithy/blob/ee0b4ff90daaaa5101f32da936c25af8c91cc6e9/smithy-aws-protocol-tests/model/restJson1/validation/malformed-length.smithy#L296-L295
                                rust("""Self::Key(key_constraint_violation) => key_constraint_violation.as_validation_exception_field(path),""")
                            }
                            if (isValueConstrained(valueShape, model, symbolProvider)) {
                                // `as_str()` works with regular `String`s and constrained string shapes.
                                rust("""Self::Value(key, value_constraint_violation) => value_constraint_violation.as_validation_exception_field(path + "/" + key.as_str()),""")
                            }
                        }
                    }
                }
            }
        }
    }
}
