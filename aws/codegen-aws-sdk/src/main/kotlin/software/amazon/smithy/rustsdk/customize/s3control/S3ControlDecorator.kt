/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.s3control

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustSettings
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointCustomization
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rustName
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rustsdk.endpoints.stripEndpointTrait
import software.amazon.smithy.rustsdk.getBuiltIn
import software.amazon.smithy.rustsdk.toWritable

class S3ControlDecorator : ClientCodegenDecorator {
    override val name: String = "S3Control"
    override val order: Byte = 0

    override fun transformModel(
        service: ServiceShape,
        model: Model,
        settings: ClientRustSettings,
    ): Model = stripEndpointTrait("AccountId")(model)

    override fun endpointCustomizations(codegenContext: ClientCodegenContext): List<EndpointCustomization> {
        return listOf(
            object : EndpointCustomization {
                override fun setBuiltInOnServiceConfig(
                    name: String,
                    value: Node,
                    configBuilderRef: String,
                ): Writable? {
                    if (!name.startsWith("AWS::S3Control")) {
                        return null
                    }
                    val builtIn = codegenContext.getBuiltIn(name) ?: return null
                    return writable {
                        rustTemplate(
                            "let $configBuilderRef = $configBuilderRef.${builtIn.name.rustName()}(#{value});",
                            "value" to value.toWritable(),
                        )
                    }
                }
            },
        )
    }
}
