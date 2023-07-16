/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.apigateway

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rustsdk.InlineAwsDependency

class ApiGatewayDecorator : ClientCodegenDecorator {
    override val name: String = "ApiGateway"
    override val order: Byte = 0

    // TODO(enableNewSmithyRuntimeCleanup): Delete when cleaning up middleware
    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> =
        baseCustomizations.letIf(codegenContext.smithyRuntimeMode.generateMiddleware) {
            it + ApiGatewayAddAcceptHeader()
        }

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> =
        baseCustomizations.letIf(codegenContext.smithyRuntimeMode.generateOrchestrator) {
            it + ApiGatewayAcceptHeaderInterceptorCustomization(codegenContext)
        }
}

// TODO(enableNewSmithyRuntimeCleanup): Delete when cleaning up middleware
private class ApiGatewayAddAcceptHeader : OperationCustomization() {
    override fun section(section: OperationSection): Writable = when (section) {
        is OperationSection.FinalizeOperation -> emptySection
        is OperationSection.OperationImplBlock -> emptySection
        is OperationSection.MutateRequest -> writable {
            rust(
                """${section.request}
                .http_mut()
                .headers_mut()
                .insert("Accept", #T::HeaderValue::from_static("application/json"));""",
                RuntimeType.Http,
            )
        }

        else -> emptySection
    }
}

private class ApiGatewayAcceptHeaderInterceptorCustomization(private val codegenContext: ClientCodegenContext) :
    ServiceRuntimePluginCustomization() {
    override fun section(section: ServiceRuntimePluginSection): Writable = writable {
        if (section is ServiceRuntimePluginSection.RegisterRuntimeComponents) {
            section.registerInterceptor(codegenContext.runtimeConfig, this) {
                rustTemplate(
                    "#{Interceptor}::default()",
                    "Interceptor" to RuntimeType.forInlineDependency(
                        InlineAwsDependency.forRustFile(
                            "apigateway_interceptors",
                            additionalDependency = arrayOf(
                                CargoDependency.smithyRuntimeApi(codegenContext.runtimeConfig),
                            ),
                        ),
                    ).resolve("AcceptHeaderInterceptor"),
                )
            }
        }
    }
}
