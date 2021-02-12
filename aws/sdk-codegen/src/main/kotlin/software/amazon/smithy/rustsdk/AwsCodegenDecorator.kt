/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.aws.traits.auth.SigV4Trait
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization

class AwsCodegenDecorator : RustCodegenDecorator {
    override val name: String = "AwsSdkCodgenDecorator"
    override val order: Byte = -1
    override fun configCustomizations(
        protocolConfig: ProtocolConfig,
        baseCustomizations: List<ConfigCustomization>
    ): List<ConfigCustomization> {
        val awsCustomizations = mutableListOf<ConfigCustomization>()
        awsCustomizations += RegionConfig(protocolConfig.runtimeConfig)
        awsCustomizations += EndpointConfigCustomization(protocolConfig)
        protocolConfig.serviceShape.getTrait(SigV4Trait::class.java).map { trait ->
            awsCustomizations += SigV4SigningConfig(trait)
        }
        return awsCustomizations + baseCustomizations
    }

    override fun operationCustomizations(
        protocolConfig: ProtocolConfig,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>
    ): List<OperationCustomization> {
        return listOf(SigV4SigningPlugin(operation, protocolConfig.runtimeConfig), EndpointConfigPlugin(protocolConfig.runtimeConfig, operation), RegionConfigPlugin(operation)) + baseCustomizations
    }
}
