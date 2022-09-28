/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.rust.codegen.client.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.client.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.client.rustlang.Visibility
import software.amazon.smithy.rust.codegen.client.rustlang.rust
import software.amazon.smithy.rust.codegen.client.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.client.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.client.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.server.smithy.PubCrateConstraintViolationSymbolProvider
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

        val constraintViolationCodegenScope = arrayOf(
            "KeyConstraintViolationSymbol" to constraintViolationSymbolProvider.toSymbol(keyShape),
            "ValueConstraintViolationSymbol" to constraintViolationSymbolProvider.toSymbol(valueShape),
            "KeySymbol" to constrainedShapeSymbolProvider.toSymbol(keyShape),
        )

        val constraintViolationVisibility = if (publicConstrainedTypes) {
            Visibility.PUBLIC
        } else {
            Visibility.PUBCRATE
        }
        modelsModuleWriter.withModule(
            constraintViolationSymbol.namespace.split(constraintViolationSymbol.namespaceDelimiter).last(),
            RustMetadata(visibility = constraintViolationVisibility),
        ) {
            // TODO(https://github.com/awslabs/smithy-rs/issues/1401) We should really have two `ConstraintViolation`
            //  types here. One will just have variants for each constraint trait on the map shape, for use by the user.
            //  The other one will have variants if the shape's key or value is directly or transitively constrained,
            //  and is for use by the framework.
            rustTemplate(
                """
                ##[derive(Debug, PartialEq)]
                pub enum $constraintViolationName {
                    ${if (shape.hasTrait<LengthTrait>()) "Length(usize)," else ""}
                    ${if (isKeyConstrained(keyShape, symbolProvider)) "##[doc(hidden)] Key(#{KeyConstraintViolationSymbol})," else ""}
                    ${if (isValueConstrained(valueShape, model, symbolProvider)) "##[doc(hidden)] Value(#{KeySymbol}, #{ValueConstraintViolationSymbol})," else ""}
                }
                """,
                *constraintViolationCodegenScope,
            )

            rustBlock("impl $constraintViolationName") {
                rustBlock("pub(crate) fn as_validation_exception_field(self, path: String) -> crate::model::ValidationExceptionField") {
                    rustBlock("match self") {
                        shape.getTrait<LengthTrait>()?.also {
                            rust(
                                """
                                Self::Length(length) => crate::model::ValidationExceptionField {
                                    message: format!("${it.validationErrorMessage()}", length, &path),
                                    path,
                                },
                                """)
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
