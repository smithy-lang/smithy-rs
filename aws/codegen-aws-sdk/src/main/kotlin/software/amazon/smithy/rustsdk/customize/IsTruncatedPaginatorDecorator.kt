/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustSettings
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.traits.IsTruncatedPaginatorTrait
import software.amazon.smithy.rust.codegen.core.util.letIf
import java.util.logging.Logger

/**
 * Decorator for adding isTruncatedPaginator trait
 */
class IsTruncatedPaginatorDecorator : ClientCodegenDecorator {
    override val name: String = "IsTruncatedPaginatorDecorator"
    override val order: Byte = 0
    private val logger: Logger = Logger.getLogger(javaClass.name)
    private val operationsWithIsTruncatedPaginator = setOf(ShapeId.from("com.amazonaws.s3#ListPartsOutput"))

    override fun transformModel(
        service: ServiceShape,
        model: Model,
        settings: ClientRustSettings,
    ): Model =
        ModelTransformer.create().mapShapes(model) { shape ->
            shape.letIf(isInIsTruncatedList(shape)) {
                logger.info("Adding IsTruncatedPaginator trait to $it")
                (it as StructureShape).toBuilder().addTrait(IsTruncatedPaginatorTrait()).build()
            }
        }

    private fun isInIsTruncatedList(shape: Shape): Boolean {
        return shape.isStructureShape && operationsWithIsTruncatedPaginator.contains(shape.id)
    }
}
