/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.auth.AuthCustomization
import software.amazon.smithy.rust.codegen.client.smithy.auth.AuthSection
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.ConditionalDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.Tracing
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.toType
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.sdkId

/**
 * Contains a list of SDK IDs that are allowed to use the endpoint-based auth scheme resolution logic.
 *
 * Going forward, we expect new services to leverage static information in a model as much as possible (e.g., the auth
 * trait). This helps avoid runtime factors, such as endpoints, influencing auth option resolution behavior.
 */
private val EndpointBasedAuthSchemeAllowList =
    setOf(
        "CloudFront KeyValueStore",
        "EventBridge",
        "S3",
        "SESv2",
    )

class EndpointBasedAuthSchemeDecorator : ConditionalDecorator(
    predicate = { codegenContext, _ ->
        codegenContext?.let {
            EndpointBasedAuthSchemeAllowList.contains(codegenContext.serviceShape.sdkId())
        } ?: false
    },
    delegateTo =
        object : ClientCodegenDecorator {
            override val name: String get() = "EndpointBasedAuthSchemeDecorator"
            override val order: Byte = 0

            override fun authCustomizations(
                codegenContext: ClientCodegenContext,
                baseCustomizations: List<AuthCustomization>,
            ): List<AuthCustomization> =
                baseCustomizations +
                    object : AuthCustomization() {
                        val ctx = codegenContext

                        override fun section(section: AuthSection) =
                            writable {
                                when (section) {
                                    is AuthSection.DefaultResolverAdditionalImpl -> {
                                        rustTemplate(
                                            """
                                            let _fut = #{AuthSchemeOptionsFuture}::new(async move {
                                                #{resolve_endpoint_based_auth_scheme_options}(
                                                    modeled_auth_options,
                                                    _cfg,
                                                    _runtime_components,
                                                ).await
                                            });
                                            """,
                                            "AuthSchemeOptionsFuture" to RuntimeType.smithyRuntimeApiClient(ctx.runtimeConfig).resolve("client::auth::AuthSchemeOptionsFuture"),
                                            "AuthSchemeOptionResolverParams" to RuntimeType.smithyRuntimeApiClient(ctx.runtimeConfig).resolve("client::auth::AuthSchemeOptionResolverParams"),
                                            "resolve_endpoint_based_auth_scheme_options" to inlineModule(ctx.runtimeConfig).resolve("resolve_endpoint_based_auth_scheme_options"),
                                        )
                                    }

                                    else -> emptySection
                                }
                            }
                    }
        },
)

private fun inlineModule(runtimeConfig: RuntimeConfig) =
    InlineAwsDependency.forRustFile(
        "endpoint_auth", visibility = Visibility.PUBCRATE,
        CargoDependency.smithyRuntimeApiClient(runtimeConfig),
        Tracing,
    ).toType()
