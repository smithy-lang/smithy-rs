/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rustsdk.InlineAwsDependency

/**
 * Decorator that tracks endpoint override business metric when endpoint URL is configured.
 */
class EndpointOverrideMetricDecorator : ClientCodegenDecorator {
    override val name: String = "EndpointOverrideMetric"
    override val order: Byte = 0

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> =
        baseCustomizations + listOf(EndpointOverrideFeatureTrackerInterceptor(codegenContext))
}

private class EndpointOverrideFeatureTrackerInterceptor(codegenContext: ClientCodegenContext) :
    ServiceRuntimePluginCustomization() {
    override fun section(section: ServiceRuntimePluginSection) =
        writable {
            if (section is ServiceRuntimePluginSection.RegisterRuntimeComponents) {
                section.registerInterceptor(this) {
                    rustTemplate(
                        "#{Interceptor}",
                        "Interceptor" to
                            RuntimeType.forInlineDependency(
                                InlineAwsDependency.forRustFile(
                                    "endpoint_override",
                                ),
                            ).resolve("EndpointOverrideFeatureTrackerInterceptor"),
                    )
                }
            }
        }
}
