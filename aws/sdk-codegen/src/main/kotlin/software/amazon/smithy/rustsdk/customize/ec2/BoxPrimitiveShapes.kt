/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.ec2

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.AbstractShapeBuilder
import software.amazon.smithy.model.shapes.BigDecimalShape
import software.amazon.smithy.model.shapes.BigIntegerShape
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.ByteShape
import software.amazon.smithy.model.shapes.DoubleShape
import software.amazon.smithy.model.shapes.FloatShape
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.shapes.LongShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShortShape
import software.amazon.smithy.model.traits.BoxTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.core.util.UNREACHABLE
import software.amazon.smithy.utils.ToSmithyBuilder

object BoxPrimitiveShapes {
    fun processModel(model: Model): Model {
        val transformer = ModelTransformer.create()
        return transformer.mapShapes(model, ::boxPrimitives)
    }

    private fun boxPrimitives(shape: Shape): Shape {
        return when (shape) {
            is NumberShape -> {
                when (shape) {
                    is ByteShape -> box(shape)
                    is DoubleShape -> box(shape)
                    is LongShape -> box(shape)
                    is ShortShape -> box(shape)
                    is FloatShape -> box(shape)
                    is BigDecimalShape -> box(shape)
                    is BigIntegerShape -> box(shape)
                    is IntegerShape -> box(shape)
                    else -> UNREACHABLE("unhandled numeric shape: $shape")
                }
            }
            is BooleanShape -> box(shape)
            else -> shape
        }
    }

    private fun <T> box(shape: T): Shape where T : Shape, T : ToSmithyBuilder<T> {
        return (shape.toBuilder() as AbstractShapeBuilder<*, T>).addTrait(BoxTrait()).build()
    }
}
