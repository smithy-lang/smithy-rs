/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

/**
 * Decorator that tracks observability business metrics when tracing/metrics providers are configured.
 */
class ObservabilityMetricDecorator : ClientCodegenDecorator {
    override val name: String = "ObservabilityMetric"
    override val order: Byte = 0

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> =
        baseCustomizations + listOf(ObservabilityFeatureTrackerInterceptor(codegenContext))
}

private class ObservabilityFeatureTrackerInterceptor(private val codegenContext: ClientCodegenContext) :
    ServiceRuntimePluginCustomization() {
    override fun section(section: ServiceRuntimePluginSection) =
        writable {
            if (section is ServiceRuntimePluginSection.RegisterRuntimeComponents) {
                section.registerInterceptor(this) {
                    val runtimeConfig = codegenContext.runtimeConfig
                    rustTemplate(
                        "#{Interceptor}",
                        "Interceptor" to
                            RuntimeType.forInlineDependency(
                                InlineAwsDependency.forRustFile(
                                    "observability_feature",
                                    Visibility.PRIVATE,
                                    CargoDependency.smithyObservability(runtimeConfig),
                                    CargoDependency.smithyObservabilityOtel(runtimeConfig),
                                ),
                            ).resolve("ObservabilityFeatureTrackerInterceptor"),
                    )
                }
            }
        }
}
