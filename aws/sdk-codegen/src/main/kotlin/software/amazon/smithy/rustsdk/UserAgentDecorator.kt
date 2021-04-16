/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.aws.traits.ServiceTrait
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.util.dq

/**
 * Inserts a UserAgent configuration into the operation
 */
class UserAgentDecorator : RustCodegenDecorator {
    override val name: String = "UserAgent"
    override val order: Byte = 10

    override fun libRsCustomizations(
        protocolConfig: ProtocolConfig,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> {
        // We are generating an AWS SDK, the service needs to have the AWS service trait
        val serviceTrait = protocolConfig.serviceShape.expectTrait(ServiceTrait::class.java)
        return baseCustomizations + ApiVersion(protocolConfig.runtimeConfig, serviceTrait)
    }

    override fun operationCustomizations(
        protocolConfig: ProtocolConfig,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>
    ): List<OperationCustomization> {
        return baseCustomizations + UserAgentFeature(protocolConfig.runtimeConfig)
    }
}

/**
 * Adds a static `API_METADATA` variable to the crate root containing the serviceId & the version of the crate for this individual service
 */
class ApiVersion(private val runtimeConfig: RuntimeConfig, serviceTrait: ServiceTrait) : LibRsCustomization() {
    private val serviceId = serviceTrait.sdkId.toLowerCase().replace(" ", "")
    override fun section(section: LibRsSection): Writable = when (section) {
        // PKG_VERSION comes from CrateVersionGenerator
        is LibRsSection.Body -> writable { rust("static API_METADATA: #1T::ApiMetadata = #1T::ApiMetadata::new(${serviceId.dq()}, PKG_VERSION);", runtimeConfig.userAgentModule()) }
        else -> emptySection
    }
}

fun RuntimeConfig.awsHttp(): CargoDependency = awsRuntimeDependency("aws-http")
fun RuntimeConfig.userAgentModule() = awsHttp().asType().copy(name = "user_agent")

class UserAgentFeature(private val runtimeConfig: RuntimeConfig) : OperationCustomization() {
    override fun section(section: OperationSection): Writable = when (section) {
        is OperationSection.MutateRequest -> writable {
            rust(
                """
                ${section.request}.config_mut().insert(#T::AwsUserAgent::new_from_environment(crate::API_METADATA.clone()));
                """,
                runtimeConfig.userAgentModule()
            )
        }
        else -> emptySection
    }
}
