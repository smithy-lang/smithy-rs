/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.apigateway

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rustsdk.InlineAwsDependency

class ApiGatewayDecorator : ClientCodegenDecorator {
    override val name: String = "ApiGateway"
    override val order: Byte = 0

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> =
        baseCustomizations + ApiGatewayAcceptHeaderInterceptorCustomization(codegenContext)
}

private class ApiGatewayAcceptHeaderInterceptorCustomization(private val codegenContext: ClientCodegenContext) :
    ServiceRuntimePluginCustomization() {
    override fun section(section: ServiceRuntimePluginSection): Writable =
        writable {
            if (section is ServiceRuntimePluginSection.RegisterRuntimeComponents) {
                section.registerInterceptor(this) {
                    rustTemplate(
                        "#{Interceptor}::default()",
                        "Interceptor" to
                            RuntimeType.forInlineDependency(
                                InlineAwsDependency.forRustFile(
                                    "apigateway_interceptors",
                                    additionalDependency =
                                        arrayOf(
                                            CargoDependency.smithyRuntimeApiClient(codegenContext.runtimeConfig),
                                            CargoDependency.Http1x,
                                        ),
                                ),
                            ).resolve("AcceptHeaderInterceptor"),
                    )
                }
            }
        }
}
