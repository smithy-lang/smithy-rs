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
import software.amazon.smithy.rust.codegen.core.util.letIf

class InvocationIdDecorator : ClientCodegenDecorator {
    override val name: String get() = "InvocationIdDecorator"
    override val order: Byte get() = 0
    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> =
        baseCustomizations.letIf(codegenContext.smithyRuntimeMode.generateOrchestrator) {
            it + listOf(InvocationIdRuntimePluginCustomization(codegenContext))
        }
}

private class InvocationIdRuntimePluginCustomization(
    private val codegenContext: ClientCodegenContext,
) : ServiceRuntimePluginCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val awsRuntime = AwsRuntimeType.awsRuntime(runtimeConfig)
    private val codegenScope = arrayOf(
        "InvocationIdInterceptor" to awsRuntime.resolve("invocation_id::InvocationIdInterceptor"),
    )

    override fun section(section: ServiceRuntimePluginSection): Writable = writable {
        if (section is ServiceRuntimePluginSection.RegisterRuntimeComponents) {
            section.registerInterceptor(codegenContext.runtimeConfig, this) {
                rustTemplate("#{InvocationIdInterceptor}::new()", *codegenScope)
            }
        }
    }
}
