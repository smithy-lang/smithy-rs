/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.traits.AuthTrait
import software.amazon.smithy.model.traits.OptionalAuthTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustSettings
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.util.shapeId

// / STS (and possibly other services) need to have auth manually set to []
class DisabledAuthDecorator() : ClientCodegenDecorator {
    override val name: String = "OptionalAuth"
    override val order: Byte = 0

    private val optionalAuth =
        mapOf(
            "com.amazonaws.sts#AWSSecurityTokenServiceV20110615".shapeId() to
                setOf(
                    "com.amazonaws.sts#AssumeRoleWithSAML".shapeId(),
                    "com.amazonaws.sts#AssumeRoleWithWebIdentity".shapeId(),
                ),
        )

    private fun applies(service: ServiceShape) =
        optionalAuth.containsKey(service.id)

    override fun transformModel(service: ServiceShape, model: Model, settings: ClientRustSettings): Model {
        if (!applies(service)) {
            return model
        }
        val optionalOperations = optionalAuth[service.id]!!
        return ModelTransformer.create().mapShapes(model) {
            if (optionalOperations.contains(it.id) && it is OperationShape) {
                if (settings.codegenConfig.enableNewSmithyRuntime.generateOrchestrator) {
                    it.toBuilder().addTrait(OptionalAuthTrait()).build()
                } else {
                    // In middleware, having an empty @auth trait completely disabled
                    // auth for an operation since not having credentials isn't an option
                    // in that implementation.
                    it.toBuilder().addTrait(AuthTrait(setOf())).build()
                }
            } else {
                it
            }
        }
    }
}
