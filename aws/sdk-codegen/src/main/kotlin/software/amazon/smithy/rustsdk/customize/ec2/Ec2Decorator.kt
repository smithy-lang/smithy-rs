/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.ec2

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.letIf

class Ec2Decorator : RustCodegenDecorator<ClientCodegenContext> {
    override val name: String = "Ec2"
    override val order: Byte = 0
    private val ec2 = ShapeId.from("com.amazonaws.ec2#AmazonEC2")

    private fun applies(serviceShape: ServiceShape) =
        serviceShape.id == ec2

    override fun transformModel(service: ServiceShape, model: Model): Model {
        // EC2 incorrectly models primitive shapes as unboxed when they actually
        // need to be boxed for the API to work properly
        return model.letIf(
            applies(service),
            BoxPrimitiveShapes::processModel
        )
    }
}
