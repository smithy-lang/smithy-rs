/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.makeRustBoxed
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.server.smithy.InlineModuleCreator
import software.amazon.smithy.rust.codegen.server.smithy.PubCrateConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.traits.ConstraintViolationRustBoxTrait
import software.amazon.smithy.rust.codegen.server.smithy.traits.isReachableFromOperationInput

class MapConstraintViolationGenerator(
    codegenContext: ServerCodegenContext,
    private val inlineModuleCreator: InlineModuleCreator,
    val shape: MapShape,
    private val validationExceptionConversionGenerator: ValidationExceptionConversionGenerator,
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
        val keyConstraintViolationExists = shape.isReachableFromOperationInput() && isKeyConstrained(keyShape, symbolProvider)
        val valueConstraintViolationExists = shape.isReachableFromOperationInput() && isValueConstrained(valueShape, model, symbolProvider)
        if (keyConstraintViolationExists) {
            constraintViolationCodegenScopeMutableList.add("KeyConstraintViolationSymbol" to constraintViolationSymbolProvider.toSymbol(keyShape))
        }
        if (valueConstraintViolationExists) {
            constraintViolationCodegenScopeMutableList.add(
                "ValueConstraintViolationSymbol" to
                    constraintViolationSymbolProvider.toSymbol(valueShape).letIf(
                        shape.value.hasTrait<ConstraintViolationRustBoxTrait>(),
                    ) {
                        it.makeRustBoxed()
                    },
            )
            constraintViolationCodegenScopeMutableList.add("KeySymbol" to constrainedShapeSymbolProvider.toSymbol(keyShape))
        }
        val constraintViolationCodegenScope = constraintViolationCodegenScopeMutableList.toTypedArray()

        val constraintViolationVisibility = if (publicConstrainedTypes) {
            Visibility.PUBLIC
        } else {
            Visibility.PUBCRATE
        }

        inlineModuleCreator(constraintViolationSymbol) {
            // TODO(https://github.com/awslabs/smithy-rs/issues/1401) We should really have two `ConstraintViolation`
            //  types here. One will just have variants for each constraint trait on the map shape, for use by the user.
            //  The other one will have variants if the shape's key or value is directly or transitively constrained,
            //  and is for use by the framework.
            rustTemplate(
                """
                ##[allow(clippy::enum_variant_names)]
                ##[derive(Debug, PartialEq)]
                pub${ if (constraintViolationVisibility == Visibility.PUBCRATE) " (crate) " else "" } enum $constraintViolationName {
                    ${if (shape.hasTrait<LengthTrait>()) "Length(usize)," else ""}
                    ${if (keyConstraintViolationExists) "##[doc(hidden)] Key(#{KeyConstraintViolationSymbol})," else ""}
                    ${if (valueConstraintViolationExists) "##[doc(hidden)] Value(#{KeySymbol}, #{ValueConstraintViolationSymbol})," else ""}
                }
                """,
                *constraintViolationCodegenScope,
            )

            if (shape.isReachableFromOperationInput()) {
                rustTemplate(
                    """
                    impl $constraintViolationName {
                        #{MapShapeConstraintViolationImplBlock}
                    }
                    """,
                    "MapShapeConstraintViolationImplBlock" to validationExceptionConversionGenerator.mapShapeConstraintViolationImplBlock(
                        shape,
                        keyShape,
                        valueShape,
                        symbolProvider,
                        model,
                    ),
                )
            }
        }
    }
}
