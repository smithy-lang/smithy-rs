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
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable

class RetryInformationHeaderDecorator : ClientCodegenDecorator {
    override val name: String = "RetryInformationHeader"
    override val order: Byte = 10

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> =
        baseCustomizations + listOf(AddRetryInformationHeaderInterceptors(codegenContext))
}

private class AddRetryInformationHeaderInterceptors(codegenContext: ClientCodegenContext) :
    ServiceRuntimePluginCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val awsRuntime = AwsRuntimeType.awsRuntime(runtimeConfig)

    override fun section(section: ServiceRuntimePluginSection): Writable =
        writable {
            if (section is ServiceRuntimePluginSection.RegisterRuntimeComponents) {
                // Track the latency between client and server.
                section.registerInterceptor(this) {
                    rust(
                        "#T::new()",
                        awsRuntime.resolve("service_clock_skew::ServiceClockSkewInterceptor"),
                    )
                }

                // Add request metadata to outgoing requests. Sets a header.
                section.registerInterceptor(this) {
                    rust("#T::new()", awsRuntime.resolve("request_info::RequestInfoInterceptor"))
                }
            }
        }
}
