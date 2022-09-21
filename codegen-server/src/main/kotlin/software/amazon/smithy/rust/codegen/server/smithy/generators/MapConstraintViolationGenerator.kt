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
import software.amazon.smithy.rust.codegen.client.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.client.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.server.smithy.PubCrateConstraintViolationSymbolProvider

class MapConstraintViolationGenerator(
    codegenContext: ServerCodegenContext,
    private val modelsModuleWriter: RustWriter,
    val shape: MapShape,
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
        val keyShape = model.expectShape(shape.key.target, StringShape::class.java)
        val valueShape = model.expectShape(shape.value.target)
        val constraintViolationSymbol = constraintViolationSymbolProvider.toSymbol(shape)
        val constraintViolationName = constraintViolationSymbol.name

        val constraintViolationCodegenScope = listOfNotNull(
            if (isKeyConstrained(keyShape, symbolProvider)) {
                "KeyConstraintViolationSymbol" to constraintViolationSymbolProvider.toSymbol(keyShape)
            } else {
                null
            },
            if (isValueConstrained(valueShape, model, symbolProvider)) {
                "ValueConstraintViolationSymbol" to constraintViolationSymbolProvider.toSymbol(valueShape)
            } else {
                null
            },
        ).toTypedArray()

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
                    ${if (isValueConstrained(valueShape, model, symbolProvider)) "##[doc(hidden)] Value(#{ValueConstraintViolationSymbol})," else ""}
                }
                """,
                *constraintViolationCodegenScope,
            )
        }
    }
}
