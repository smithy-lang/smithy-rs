/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.ec2

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustSettings
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator

class Ec2Decorator : ClientCodegenDecorator {
    override val name: String = "Ec2"
    override val order: Byte = 0

    // EC2 incorrectly models primitive shapes as unboxed when they actually
    // need to be boxed for the API to work properly
    override fun transformModel(
        service: ServiceShape,
        model: Model,
        settings: ClientRustSettings,
    ): Model = EC2MakePrimitivesOptional.processModel(model)
}
