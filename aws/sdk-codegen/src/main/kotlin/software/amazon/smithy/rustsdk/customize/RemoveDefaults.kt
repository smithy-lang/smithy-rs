/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.AbstractShapeBuilder
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.DefaultTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.utils.ToSmithyBuilder
import java.util.logging.Logger

/**
 * Removes default values from specified root shapes, and any members that target those
 * root shapes.
 */
object RemoveDefaults {
    private val logger: Logger = Logger.getLogger(javaClass.name)

    fun processModel(model: Model, removeDefaultsFrom: Set<ShapeId>): Model {
        val removedRootDefaults: MutableSet<ShapeId> = HashSet()
        val removedRootDefaultsModel = ModelTransformer.create().mapShapes(model) { shape ->
            shape.letIf(shouldRemoveRootDefault(shape, removeDefaultsFrom)) {
                logger.info("Removing default trait from root $shape")
                removedRootDefaults.add(shape.id)
                removeDefault(shape)
            }
        }

        return ModelTransformer.create().mapShapes(removedRootDefaultsModel) { shape ->
            shape.letIf(shouldRemoveMemberDefault(shape, removedRootDefaults)) {
                logger.info("Removing default trait from member $shape")
                removeDefault(shape)
            }
        }
    }

    private fun shouldRemoveRootDefault(shape: Shape, removeDefaultsFrom: Set<ShapeId>): Boolean {
        return shape !is MemberShape && removeDefaultsFrom.contains(shape.id) && shape.hasTrait<DefaultTrait>()
    }

    private fun shouldRemoveMemberDefault(shape: Shape, removeDefaultsFrom: Set<ShapeId>): Boolean {
        return shape is MemberShape && removeDefaultsFrom.contains(shape.target) && shape.hasTrait<DefaultTrait>()
    }

    private fun removeDefault(shape: Shape): Shape {
        return ((shape as ToSmithyBuilder<*>).toBuilder() as AbstractShapeBuilder<*, *>)
            .removeTrait(DefaultTrait.ID)
            .build()
    }
}
