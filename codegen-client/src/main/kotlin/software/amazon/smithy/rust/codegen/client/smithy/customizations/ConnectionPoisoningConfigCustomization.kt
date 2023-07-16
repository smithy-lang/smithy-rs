/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.smithyRuntime

class ConnectionPoisoningRuntimePluginCustomization(
    codegenContext: ClientCodegenContext,
) : ServiceRuntimePluginCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig

    override fun section(section: ServiceRuntimePluginSection): Writable = writable {
        when (section) {
            is ServiceRuntimePluginSection.RegisterRuntimeComponents -> {
                // This interceptor assumes that a compatible Connector is set. Otherwise, connection poisoning
                // won't work and an error message will be logged.
                section.registerInterceptor(runtimeConfig, this) {
                    rust(
                        "#T::new()",
                        smithyRuntime(runtimeConfig).resolve("client::connectors::connection_poisoning::ConnectionPoisoningInterceptor"),
                    )
                }
            }

            else -> emptySection
        }
    }
}
