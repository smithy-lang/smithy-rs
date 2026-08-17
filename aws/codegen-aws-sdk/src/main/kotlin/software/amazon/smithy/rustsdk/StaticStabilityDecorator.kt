/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

/**
 * Wires up static stability for AWS clients — the identity cache that owns consistent credential
 * refresh, plus the interceptor that invalidates credentials a target service rejects.
 *
 * On the generated `ServiceRuntimePlugin`, it installs `StaticStabilityCache` as the default
 * identity cache for clients built directly from a `Config` (without `aws-config`) — only when the
 * customer hasn't configured one, and only at `BehaviorVersion >= v2026_08_01` (older versions keep
 * `LazyCache`). Clients built through `aws-config` get the same cache from `ConfigLoader::load()`.
 *
 * On every operation, it registers `CredentialAuthFailureInterceptor`: when a target service
 * rejects a request with `ExpiredToken`/`InvalidToken`, the interceptor sets a data-free config-bag
 * marker, and the orchestrator invalidates the signing identity so the next resolution refreshes.
 */
class StaticStabilityDecorator : ClientCodegenDecorator {
    override val name: String = "StaticStability"
    override val order: Byte = 0

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> = baseCustomizations + StaticStabilityCacheCustomization(codegenContext)

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> =
        baseCustomizations + CredentialAuthFailureInterceptorCustomization(codegenContext, operation)
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

private class CredentialAuthFailureInterceptorCustomization(
    codegenContext: ClientCodegenContext,
    private val operation: OperationShape,
) : OperationCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val symbolProvider = codegenContext.symbolProvider

    override fun section(section: OperationSection) =
        when (section) {
            is OperationSection.AdditionalInterceptors ->
                writable {
                    section.registerInterceptor(runtimeConfig, this) {
                        rustTemplate(
                            "#{CredentialAuthFailureInterceptor}::<#{OperationError}>::new()",
                            "CredentialAuthFailureInterceptor" to
                                AwsRuntimeType.awsRuntime(runtimeConfig)
                                    .resolve("static_stability::invalidation::CredentialAuthFailureInterceptor"),
                            "OperationError" to symbolProvider.symbolForOperationError(operation),
                        )
                    }
                }

            else -> emptySection
        }
}
