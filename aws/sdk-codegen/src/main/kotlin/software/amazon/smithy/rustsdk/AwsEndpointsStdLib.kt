/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameter
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.CustomRuntimeFunction
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointCustomization
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen.awsStandardLib
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import kotlin.io.path.readText


class AwsEndpointsStdLib() : RustCodegenDecorator<ClientProtocolGenerator, ClientCodegenContext> {
    private var partitionsCache: ObjectNode? = null
    override val name: String = "AwsEndpointsStdLib"
    override val order: Byte = 0

    override fun supportsCodegenContext(clazz: Class<out CodegenContext>): Boolean {
        return clazz.isAssignableFrom(ClientCodegenContext::class.java)
    }

    private fun partitionsDotjson(sdkSettings: SdkSettings): ObjectNode {
        if (partitionsCache == null) {
            val partitionsJson = when (val path = sdkSettings.partitionsDotJson) {
                null -> (
                    javaClass.getResource("/default-partitions.json")
                        ?: throw IllegalStateException("Failed to find default-default-partitions.json in the JAR")
                    ).readText()

                else -> path.readText()
            }
            partitionsCache = Node.parse(partitionsJson).expectObjectNode()
        }
        return partitionsCache!!
    }

    override fun endpointCustomizations(codegenContext: ClientCodegenContext): List<EndpointCustomization> {
        return listOf<EndpointCustomization>(
            object : EndpointCustomization {
                override fun builtInDefaultValue(parameter: Parameter, configRef: String): Writable? = null

                override fun customRuntimeFunctions(codegenContext: ClientCodegenContext): List<CustomRuntimeFunction> {
                    val sdkSettings = SdkSettings.from(codegenContext.settings)
                    return awsStandardLib(codegenContext.runtimeConfig, partitionsDotjson(sdkSettings))
                }
            },
        )
    }
}
