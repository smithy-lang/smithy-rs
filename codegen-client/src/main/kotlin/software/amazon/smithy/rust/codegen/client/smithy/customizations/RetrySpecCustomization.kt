/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope

/**
 * Stores a [RetrySpec] in the service runtime plugin config based on the
 * client's [BehaviorVersion]. This is done in codegen (not in the runtime's
 * default_plugins) so that old SDKs paired with a new runtime don't
 * inadvertently pick up new retry behavior.
 */
class RetrySpecCustomization(private val codegenContext: ClientCodegenContext) :
    ServiceRuntimePluginCustomization() {
    override fun section(section: ServiceRuntimePluginSection): Writable =
        writable {
            if (section is ServiceRuntimePluginSection.AdditionalConfig) {
                val rc = codegenContext.runtimeConfig
                rustTemplate(
                    """
                    if let #{Some}(bv) = ${section.serviceConfigName}.behavior_version.as_ref() {
                        ${section.newLayerName}.store_put(
                            #{RetryConfig}::standard()
                                .with_retry_spec(#{RetrySpec}::from(*bv))
                        );
                    }
                    """,
                    *preludeScope,
                    "RetryConfig" to CargoDependency.smithyTypes(rc).toType().resolve("retry::RetryConfig"),
                    "RetrySpec" to CargoDependency.smithyTypes(rc).toType().resolve("retry::RetrySpec"),
                )
            }
        }
}
