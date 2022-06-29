/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.auth

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.AuthTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator

private fun String.shapeId() = ShapeId.from(this)
// / STS (and possibly other services) need to have auth manually set to []
class DisabledAuthDecorator : RustCodegenDecorator<ClientCodegenContext> {
    override val name: String = "OptionalAuth"
    override val order: Byte = 0

    private val optionalAuth =
        mapOf(
            "com.amazonaws.sts#AWSSecurityTokenServiceV20110615".shapeId() to
                setOf(
                    "com.amazonaws.sts#AssumeRoleWithSAML".shapeId(),
                    "com.amazonaws.sts#AssumeRoleWithWebIdentity".shapeId()
                )
        )

    private fun applies(service: ServiceShape) =
        optionalAuth.containsKey(service.id)

    override fun transformModel(service: ServiceShape, model: Model): Model {
        if (!applies(service)) {
            return model
        }
        val optionalOperations = optionalAuth[service.id]!!
        return ModelTransformer.create().mapShapes(model) {
            if (optionalOperations.contains(it.id) && it is OperationShape) {
                it.toBuilder().addTrait(AuthTrait(setOf())).build()
            } else {
                it
            }
        }
    }
}
