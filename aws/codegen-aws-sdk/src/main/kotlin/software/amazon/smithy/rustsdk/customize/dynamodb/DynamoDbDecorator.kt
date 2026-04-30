/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.dynamodb

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

class DynamoDbDecorator : ClientCodegenDecorator {
    override val name: String = "DynamoDb"
    override val order: Byte = 0

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> =
        baseCustomizations + DynamoDbServiceRuntimePluginCustomization(codegenContext)
}

private class DynamoDbServiceRuntimePluginCustomization(private val codegenContext: ClientCodegenContext) :
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
                        .is_some_and(|bv| bv.is_at_least(#{BehaviorVersion}::v2026_05_15()))
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
