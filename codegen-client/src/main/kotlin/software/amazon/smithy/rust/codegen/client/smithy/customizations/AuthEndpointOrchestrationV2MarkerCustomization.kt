/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.smithyRuntime

class AuthEndpointOrchestrationV2MarkerCustomization(codegenContext: ClientCodegenContext) :
    ServiceRuntimePluginCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig

    override fun section(section: ServiceRuntimePluginSection) =
        writable {
            when (section) {
                is ServiceRuntimePluginSection.AdditionalConfig -> {
                    section.putConfigValue(
                        this,
                        writable {
                            rust(
                                "#T",
                                smithyRuntime(runtimeConfig).resolve("client::orchestrator::AuthSchemeAndEndpointOrchestrationV2"),
                            )
                        },
                    )
                }

                else -> emptySection
            }
        }
}
