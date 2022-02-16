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

/**
 * Add at least one error to all operations in the model.
 *
 * When this decorator is applied, operations that do not have a Smithy error attatched,
 * will return `Result<OperationOutput, InternalServerError>`.
 *
 * To enable this decorator, create a file called `META-INF/services/software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator`
 * containing `codegen-server/src/main/resources/META-INF/services/software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator`.
 */
class FallibleOperationsDecorator : RustCodegenDecorator {
    override val name: String = "FallibleOperations"
    override val order: Byte = 0

    override fun transformModel(service: ServiceShape, model: Model): Model {
        val namespace = service.id.getNamespace()
        val message = MemberShape.builder().id("$namespace#InternalServerError\$message")
            .target("smithy.api#String").addTrait(RequiredTrait()).build()
        val errorShape = StructureShape.builder().id("$namespace#InternalServerError")
            .addTrait(ErrorTrait("server")).addMember(message).build()
        val modelShapes = model.toBuilder().addShapes(listOf(errorShape)).build()
        return ModelTransformer.create().mapShapes(modelShapes) { shape ->
            if (shape is OperationShape && shape.errors.isEmpty()) {
                shape.toBuilder().addError(errorShape).build()
            } else {
                shape
            }
        }
    }
}
