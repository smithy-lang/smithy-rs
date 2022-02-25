/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.traits.RequiredTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import java.util.logging.Logger

/**
 * Add an internal server error to all operations in the model.
 *
 * When this decorator is applied, even operations that do not have a Smithy error attatched,
 * will return `Result<OperationOutput, InternalServerError>`.
 *
 * To enable this decorator, create a file called 
 * `src/main/resources/META-INF/services/software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator`
 * containing the decorator class name -
 * `software.amazon.smithy.rust.codegen.server.smithy.customizations.InternalServerErrorDecorator`.
 */
class InternalServerErrorDecorator : RustCodegenDecorator {
    private val logger = Logger.getLogger(javaClass.name)
    override val name: String = "FallibleOperations"
    override val order: Byte = 0

    override fun transformModel(service: ServiceShape, model: Model): Model {
        logger.info("Applying InternalServerErrorDecorator for ${service.id}")
        val errorShape = internalServerError(service.id.getNamespace())
        val modelShapes = model.toBuilder().addShapes(listOf(errorShape)).build()
        return ModelTransformer.create().mapShapes(modelShapes) { shape ->
            if (shape is OperationShape) {
                shape.toBuilder().addError(errorShape).build()
            } else {
                shape
            }
        }
    }

    private fun internalServerError(namespace: String): StructureShape {
        return StructureShape.builder().id("$namespace#InternalServerError")
            .addTrait(ErrorTrait("server"))
            .addMember(
                MemberShape.builder()
                    .id("$namespace#InternalServerError\$message")
                    .target("smithy.api#String")
                    .addTrait(RequiredTrait())
                    .build()
            ).build()
    }
}
