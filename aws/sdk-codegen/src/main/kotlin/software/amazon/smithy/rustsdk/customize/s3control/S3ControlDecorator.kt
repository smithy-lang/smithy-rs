/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.s3control

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.EndpointTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator

class S3ControlDecorator : ClientCodegenDecorator {
    override val name: String = "S3Control"
    override val order: Byte = 0
    private fun applies(service: ServiceShape) =
        service.id == ShapeId.from("com.amazonaws.s3control#AWSS3ControlServiceV20180820")

    override fun transformModel(service: ServiceShape, model: Model): Model {
        if (!applies(service)) {
            return model
        }
        return ModelTransformer.create()
            .removeTraitsIf(model) { _, trait ->
                trait is EndpointTrait && trait.hostPrefix.labels.any {
                    it.isLabel && it.content == "AccountId"
                }
            }
    }
}
