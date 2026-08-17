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
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

/**
 * Installs `StaticStabilityCache` as the default identity cache for AWS clients built directly from
 * a `Config` (that is, without `aws-config`).
 *
 * It is registered on the generated `ServiceRuntimePlugin` and installs the cache only when the
 * customer has not already configured one. It is gated on `BehaviorVersion >= v2026_08_01`, so
 * older behavior versions keep `LazyCache`.
 *
 * Clients built through `aws-config` instead receive the same cache from `ConfigLoader::load()`.
 */
class StaticStabilityCacheDecorator : ClientCodegenDecorator {
    override val name: String = "StaticStabilityCache"
    override val order: Byte = 0

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> = baseCustomizations + StaticStabilityCacheCustomization(codegenContext)
}

private class StaticStabilityCacheCustomization(
    codegenContext: ClientCodegenContext,
) : ServiceRuntimePluginCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            "StaticStabilityCache" to
                AwsRuntimeType.awsRuntime(runtimeConfig)
                    .resolve("static_stability::StaticStabilityCache"),
            "BehaviorVersion" to
                RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                    .resolve("client::behavior_version::BehaviorVersion"),
        )

    override fun section(section: ServiceRuntimePluginSection): Writable =
        writable {
            when (section) {
                // Runs inside `ServiceRuntimePlugin::new`, where the `runtime_components` builder
                // and the service `Config` are in scope.
                is ServiceRuntimePluginSection.RegisterRuntimeComponents -> {
                    rustTemplate(
                        """
                        // Install the default only when the customer hasn't configured a cache, so
                        // an explicit `.identity_cache(..)` (e.g. the assume-role provider's
                        // `no_cache()`) is kept. Gated on the behavior version.
                        if ${section.serviceConfigName}.identity_cache().is_none()
                            && ${section.serviceConfigName}
                                .behavior_version
                                .expect("behavior version is set before the service runtime plugin runs")
                                .is_at_least(#{BehaviorVersion}::v2026_08_01())
                        {
                            runtime_components
                                .set_identity_cache(Some(#{StaticStabilityCache}::builder().build()));
                        }
                        """,
                        *codegenScope,
                    )
                }

                else -> {}
            }
        }
}
