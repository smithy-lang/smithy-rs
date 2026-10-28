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
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization

/**
 * Overrides the default retry config for DynamoDB and DynamoDB Streams. They use a 25ms
 * non-throttling initial backoff and 4 max attempts. The generic Retry Behavior 2.1 defaults are
 * 50ms and 3.
 */
class RetryDefaultOverrideDecorator : ClientCodegenDecorator {
    override val name: String = "RetryDefaultOverride"
    override val order: Byte = 0

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> =
        baseCustomizations + RetryDefaultOverrideRuntimePluginCustomization(codegenContext)

    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> {
        val rc = codegenContext.runtimeConfig
        return listOf(
            adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
                rustTemplate(
                    """
                    ##[allow(deprecated)]
                    if let #{Some}(existing_rc) = ${section.sdkConfig}.retry_config().cloned() {
                        if ${section.sdkConfig}.behavior_version().is_some_and(|bv| bv.is_at_least(#{BehaviorVersion}::v2026_11_03())) {
                            let mut rc = existing_rc.with_retry_spec(
                                #{RetrySpec}::v2_1()
                                    .with_non_throttling_initial_backoff(#{Duration}::from_millis(25))
                            );
                            let origin = ${section.sdkConfig}.get_origin("retry_config");
                            let max_attempts_configured = origin.is_client_config()
                                || origin == #{Origin}::shared_environment_variable()
                                || origin == #{Origin}::shared_profile_file();
                            if !max_attempts_configured {
                                rc = rc.with_max_attempts(4);
                            }
                            ${section.serviceConfigBuilder} = ${section.serviceConfigBuilder}.retry_config(rc);
                        }
                    }
                    """,
                    "Some" to RuntimeType.preludeScope.first { it.first == "Some" }.second,
                    "BehaviorVersion" to
                        CargoDependency.smithyRuntimeApiClient(rc).toType()
                            .resolve("client::behavior_version::BehaviorVersion"),
                    "Origin" to AwsRuntimeType.awsTypes(rc).resolve("origin::Origin"),
                    "RetryConfig" to CargoDependency.smithyTypes(rc).toType().resolve("retry::RetryConfig"),
                    "RetrySpec" to CargoDependency.smithyTypes(rc).toType().resolve("retry::RetrySpec"),
                    "Duration" to RuntimeType.Duration,
                )
            },
        )
    }
}

private class RetryDefaultOverrideRuntimePluginCustomization(private val codegenContext: ClientCodegenContext) :
    ServiceRuntimePluginCustomization() {
    override fun section(section: ServiceRuntimePluginSection): Writable =
        writable {
            if (section is ServiceRuntimePluginSection.AdditionalConfig) {
                val rc = codegenContext.runtimeConfig
                rustTemplate(
                    """
                    ##[allow(deprecated)]
                    if ${section.serviceConfigName}.behavior_version
                        .as_ref()
                        .is_some_and(|bv| bv.is_at_least(#{BehaviorVersion}::v2026_11_03()))
                    {
                        ${section.newLayerName}.store_put(
                            #{RetryConfig}::standard()
                                .with_max_attempts(4)
                                .with_retry_spec(
                                    #{RetrySpec}::v2_1()
                                        .with_non_throttling_initial_backoff(#{Duration}::from_millis(25))
                                )
                        );
                    }
                    """,
                    *preludeScope,
                    "BehaviorVersion" to
                        CargoDependency.smithyRuntimeApiClient(rc).toType()
                            .resolve("client::behavior_version::BehaviorVersion"),
                    "RetryConfig" to CargoDependency.smithyTypes(rc).toType().resolve("retry::RetryConfig"),
                    "RetrySpec" to CargoDependency.smithyTypes(rc).toType().resolve("retry::RetrySpec"),
                    "Duration" to RuntimeType.Duration,
                )
            }
        }
}
