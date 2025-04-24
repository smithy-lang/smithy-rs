/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rulesengine.language.EndpointRuleSet
import software.amazon.smithy.rulesengine.traits.EndpointRuleSetTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.AuthSchemeOption
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.ConditionalDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.AuthSchemeLister
import software.amazon.smithy.rust.codegen.client.smithy.generators.AuthOptionsPluginGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.Tracing
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.toType
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.sdkId

/**
 * Contains a list of SDK IDs that are allowed to use `EndpointBasedAuthSchemeOptionResolver`
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

class EndpointBasedAuthSchemeResolverDecorator : ConditionalDecorator(
    predicate = { codegenContext, _ ->
        codegenContext?.let {
            if (EndpointBasedAuthSchemeAllowList.contains(codegenContext.serviceShape.sdkId())) {
                true
            } else {
                // TODO(https://github.com/smithy-lang/smithy-rs/issues/4076): Remove this else once the task has
                //  been completed.
                // Although we'd like to restrict the usage of this decorator to the services listed above,
                // some services still define "sigv4a" in their endpoint rules.
                // If these services use `StaticBasedAuthSchemeOptionResolver`, the code generator currently does NOT
                // prioritize "sigv4a" as the first authentication scheme option; it defaults to "sigv4", instead.
                val endpointAuthSchemes =
                    codegenContext.serviceShape.getTrait<EndpointRuleSetTrait>()?.ruleSet?.let {
                        EndpointRuleSet.fromNode(
                            it,
                        )
                    }
                        ?.also { it.typeCheck() }?.let { AuthSchemeLister.authSchemesForRuleset(it) } ?: setOf()
                endpointAuthSchemes.contains("sigv4a")
            }
        } ?: false
    },
    delegateTo =
        object : ClientCodegenDecorator {
            override val name: String get() = "EndpointBasedAuthSchemeResolverDecorator"
            override val order: Byte = 0

            override fun authOptions(
                codegenContext: ClientCodegenContext,
                operationShape: OperationShape,
                baseAuthSchemeOptions: List<AuthSchemeOption>,
            ): List<AuthSchemeOption> =
                baseAuthSchemeOptions +
                    AuthSchemeOption.EndpointBasedAuthSchemeOption

            override fun operationCustomizations(
                codegenContext: ClientCodegenContext,
                operation: OperationShape,
                baseCustomizations: List<OperationCustomization>,
            ): List<OperationCustomization> =
                baseCustomizations +
                    object : OperationCustomization() {
                        override fun section(section: OperationSection): Writable {
                            return when (section) {
                                is OperationSection.AdditionalRuntimePlugins ->
                                    writable {
                                        rustTemplate(
                                            ".with_client_plugin(#{auth_plugin})",
                                            "auth_plugin" to
                                                AuthOptionsPluginGenerator(codegenContext).authPlugin(
                                                    InlineAwsDependency.forRustFile(
                                                        "endpoint_auth_plugin", visibility = Visibility.PUBCRATE,
                                                        CargoDependency.smithyRuntimeApiClient(codegenContext.runtimeConfig),
                                                        Tracing,
                                                    ).toType()
                                                        .resolve("EndpointBasedAuthOptionsPlugin"),
                                                    section.operationShape,
                                                    section.authSchemeOptions,
                                                ),
                                        )
                                    }

                                else -> emptySection
                            }
                        }
                    }
        },
)
