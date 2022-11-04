/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.servicedecorators.ec2

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolGenerator
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.util.letIf

private val Ec2 = ShapeId.from("com.amazonaws.ec2#AmazonEC2")

class Ec2Decorator : RustCodegenDecorator<ClientProtocolGenerator, ClientCodegenContext> {
    override val order: Byte = 0

    private fun applies(serviceShape: ServiceShape) = serviceShape.id == Ec2

    override fun transformModel(service: ServiceShape, model: Model): Model {
        // EC2 incorrectly models primitive shapes as unboxed when they actually
        // need to be boxed for the API to work properly
        return model.letIf(
            applies(service),
            Ec2MakePrimitivesOptional::processModel,
        )
    }

    override fun supportsCodegenContext(clazz: Class<out CodegenContext>): Boolean =
        clazz.isAssignableFrom(ClientCodegenContext::class.java)
}
