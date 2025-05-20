/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.AuthOptionsPluginGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope

class AuthSchemeOptionResolverDecorator : ClientCodegenDecorator {
    override val name: String get() = "AuthOptionSchemeResolverDecorator"
    override val order: Byte = 0

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> =
        baseCustomizations + listOf(AuthSchemeOptionResolverServiceRuntimePlugin(codegenContext))

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> =
        baseCustomizations + listOf(AuthSchemeOptionResolverOperationRuntimePlugin(codegenContext))
}

private class AuthSchemeOptionResolverServiceRuntimePlugin(codegenContext: ClientCodegenContext) : ServiceRuntimePluginCustomization() {
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "StaticAuthSchemeOptionResolver" to RuntimeType.smithyRuntimeApiClient(codegenContext.runtimeConfig).resolve("client::auth::static_resolver::StaticAuthSchemeOptionResolver"),
        )

    override fun section(section: ServiceRuntimePluginSection): Writable =
        writable {
            when (section) {
                is ServiceRuntimePluginSection.RegisterRuntimeComponents -> {
                    section.registerAuthSchemeOptionResolver(
                        this,
                    ) {
                        rustTemplate("#{StaticAuthSchemeOptionResolver}::new(#{Vec}::new())", *codegenScope)
                    }
                }

                else -> emptySection
            }
        }
}

private class AuthSchemeOptionResolverOperationRuntimePlugin(private val codegenContext: ClientCodegenContext) : OperationCustomization() {
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "AuthSchemeOptionResolverParams" to RuntimeType.smithyRuntimeApiClient(codegenContext.runtimeConfig).resolve("client::auth::AuthSchemeOptionResolverParams"),
        )

    override fun section(section: OperationSection): Writable =
        writable {
            when (section) {
                is OperationSection.AdditionalRuntimePluginConfig -> {
                    rustTemplate(
                        """
                        ${section.newLayerName}.store_put(#{AuthSchemeOptionResolverParams}::new(#{actual_auth_schemes:W}));
                        """,
                        *codegenScope,
                        "actual_auth_schemes" to
                            AuthOptionsPluginGenerator(codegenContext).authPlugin(
                                section.operationShape,
                                section.authSchemeOptions,
                            ),
                    )
                }

                else -> emptySection
            }
        }
}
