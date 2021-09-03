/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rustsdk.traits.PresignableTrait

private val PRESIGNABLE_OPERATIONS = listOf(
    // TODO(PresignedReqPrototype): Add the other presignable operations
    ShapeId.from("com.amazonaws.s3#GetObject"),
    ShapeId.from("com.amazonaws.s3#PutObject"),
)

// TODO(PresignedReqPrototype): Write unit test
class AwsPresigningDecorator : RustCodegenDecorator {
    override val name: String = "AwsPresigning"
    override val order: Byte = 0

    /** Adds presignable trait to known presignable operations */
    override fun transformModel(service: ServiceShape, model: Model): Model {
        return ModelTransformer.create().mapShapes(model) { shape ->
            if (shape is OperationShape && PRESIGNABLE_OPERATIONS.contains(shape.id)) {
                shape.toBuilder().addTrait(PresignableTrait()).build()
            } else {
                shape
            }
        }
    }
}
