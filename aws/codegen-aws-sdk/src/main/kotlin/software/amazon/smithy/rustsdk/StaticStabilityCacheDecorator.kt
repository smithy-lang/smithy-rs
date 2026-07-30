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

/**
 * Installs [`StaticStabilityCache`] as the default identity cache for AWS clients built *without*
 * `aws-config` (Route B: `Client::from_conf(Config::builder()...build())`).
 *
 * It is registered on the generated `ServiceRuntimePlugin`, which runs at `Order::Defaults`, so it
 * overrides the generic smithy `LazyCache` default but still loses to an explicit customer
 * `.identity_cache(..)` (which lands in the `Order::Overrides` layer). It is installed
 * unconditionally (D-NOBV) — `StaticStabilityCache` is the default AWS identity cache, not gated
 * behind a new `BehaviorVersion`.
 *
 * `aws-config` clients (Route A) get the cache from `ConfigLoader::load()` instead, which outranks
 * this (same type, same result).
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
    private val codegenContext: ClientCodegenContext,
) : ServiceRuntimePluginCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            "StaticStabilityCache" to
                AwsRuntimeType.awsRuntime(runtimeConfig)
                    .resolve("static_stability::StaticStabilityCache"),
        )

    override fun section(section: ServiceRuntimePluginSection): Writable =
        writable {
            when (section) {
                // Runs inside `ServiceRuntimePlugin::new`, where `runtime_components`
                // (a &mut RuntimeComponentsBuilder) is in scope.
                is ServiceRuntimePluginSection.RegisterRuntimeComponents -> {
                    rustTemplate(
                        """
                        // Order::Defaults -> overrides the smithy LazyCache default; a customer's
                        // explicit .identity_cache(..) (Order::Overrides) still wins. Installed
                        // unconditionally (D-NOBV): the default AWS identity cache.
                        runtime_components
                            .set_identity_cache(Some(#{StaticStabilityCache}::builder().build()));
                        """,
                        *codegenScope,
                    )
                }

                else -> {}
            }
        }
}
