/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.endpoints

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointCustomization
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.CustomRuntimeFunction
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen.awsStandardLib
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rustsdk.SdkSettings
import kotlin.io.path.readText

/**
 * Standard library functions for AWS-specific endpoint standard library functions.
 *
 * This decorator uses partitions.json to source a default partition map for the partition resolver (when used).
 *
 * For test purposes, [awsStandardLib] can be used directly with a manually supplied partitions.json
 */
class AwsEndpointsStdLib() : ClientCodegenDecorator {
    private var partitionsCache: ObjectNode? = null
    override val name: String = "AwsEndpointsStdLib"
    override val order: Byte = 0

    private fun partitionMetadata(sdkSettings: SdkSettings): ObjectNode {
        if (partitionsCache == null) {
            val partitionsJson =
                when (val path = sdkSettings.partitionsConfigPath) {
                    null -> {
                        if (sdkSettings.awsSdkBuild) {
                            PANIC("cannot use hardcoded partitions in AWS SDK build")
                        }
                        (
                            javaClass.getResource("/default-partitions.json")
                                ?: throw IllegalStateException("Failed to find default-partitions.json in the JAR")
                        ).readText()
                    }

                    else -> path.readText()
                }
            partitionsCache = Node.parse(partitionsJson).expectObjectNode()
        }
        return partitionsCache!!
    }

    override fun endpointCustomizations(codegenContext: ClientCodegenContext): List<EndpointCustomization> {
        return listOf<EndpointCustomization>(
            object : EndpointCustomization {
                override fun customRuntimeFunctions(codegenContext: ClientCodegenContext): List<CustomRuntimeFunction> {
                    val sdkSettings = SdkSettings.from(codegenContext.settings)
                    return awsStandardLib(codegenContext.runtimeConfig, partitionMetadata(sdkSettings))
                }
            },
        )
    }
}
