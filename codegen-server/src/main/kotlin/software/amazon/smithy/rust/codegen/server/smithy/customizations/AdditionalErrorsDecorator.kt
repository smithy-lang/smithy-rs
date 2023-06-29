/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
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
import software.amazon.smithy.rust.codegen.core.smithy.transformers.allErrors
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustSettings
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator

/**
 * Add at least one error to all operations in the model.
 *
 * When this decorator is applied, even operations that do not have a Smithy error attached,
 * will return `Result<OperationOutput, OperationError>`.
 *
 * To enable this decorator write its class name to a resource file like this:
 * ```
 * C="software.amazon.smithy.rust.codegen.server.smithy.customizations.AddInternalServerErrorToInfallibleOperationsDecorator"
 * F="software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator"
 * D="codegen-server/src/main/resources/META-INF/services"
 * mkdir -p "$D" && echo "$C" > "$D/$F"
 * ```
 */
class AddInternalServerErrorToInfallibleOperationsDecorator : ServerCodegenDecorator {
    override val name: String = "AddInternalServerErrorToInfallibleOperations"
    override val order: Byte = 0

    override fun transformModel(service: ServiceShape, model: Model, settings: ServerRustSettings): Model =
        addErrorShapeToModelOperations(service, model) { shape -> shape.allErrors(model).isEmpty() }
}

/**
 * Add an internal server error to all operations in the model.
 *
 * When a server for an API is subject to internal errors, for example underlying network problems,
 * and there is no native mapping of these actual errors to the API errors, servers can generate
 * the code with this decorator to add an internal error shape on-the-fly to all the operations.
 *
 * When this decorator is applied, even operations that do not have a Smithy error attached,
 * will return `Result<OperationOutput, OperationError>`.
 *
 * To enable this decorator write its class name to a resource file like this:
 * ```
 * C="software.amazon.smithy.rust.codegen.server.smithy.customizations.AddInternalServerErrorToAllOperationsDecorator"
 * F="software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator"
 * D="codegen-server/src/main/resources/META-INF/services"
 * mkdir -p "$D" && echo "$C" > "$D/$F"
 * ```
 */
class AddInternalServerErrorToAllOperationsDecorator : ServerCodegenDecorator {
    override val name: String = "AddInternalServerErrorToAllOperations"
    override val order: Byte = 0

    override fun transformModel(service: ServiceShape, model: Model, settings: ServerRustSettings): Model =
        addErrorShapeToModelOperations(service, model) { true }
}

fun addErrorShapeToModelOperations(service: ServiceShape, model: Model, opSelector: (OperationShape) -> Boolean): Model {
    val errorShape = internalServerError(service.id.namespace)
    val modelShapes = model.toBuilder().addShapes(listOf(errorShape)).build()
    return ModelTransformer.create().mapShapes(modelShapes) { shape ->
        if (shape is OperationShape && opSelector(shape)) {
            shape.toBuilder().addError(errorShape).build()
        } else {
            shape
        }
    }
}

private fun internalServerError(namespace: String): StructureShape =
    StructureShape.builder().id("$namespace#InternalServerError")
        .addTrait(ErrorTrait("server"))
        .addMember(
            MemberShape.builder()
                .id("$namespace#InternalServerError\$message")
                .target("smithy.api#String")
                .addTrait(RequiredTrait())
                .build(),
        ).build()
