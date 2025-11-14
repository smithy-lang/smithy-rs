/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable

/**
 * Registers the EndpointOverrideInterceptor to detect custom endpoint usage for business metrics
 */
class EndpointOverrideDecorator : ClientCodegenDecorator {
    override val name: String = "EndpointOverride"
    override val order: Byte = 0

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> =
        baseCustomizations + EndpointOverrideRuntimePluginCustomization(codegenContext)

    private class EndpointOverrideRuntimePluginCustomization(codegenContext: ClientCodegenContext) :
        ServiceRuntimePluginCustomization() {
        private val runtimeConfig = codegenContext.runtimeConfig
        private val awsRuntime = AwsRuntimeType.awsRuntime(runtimeConfig)

        override fun section(section: ServiceRuntimePluginSection): Writable =
            writable {
                when (section) {
                    is ServiceRuntimePluginSection.RegisterRuntimeComponents -> {
                        section.registerInterceptor(this) {
                            rust(
                                "#T::new()",
                                awsRuntime.resolve("endpoint_override::EndpointOverrideInterceptor"),
                            )
                        }
                    }
                    else -> emptySection
                }
            }
    }
}
