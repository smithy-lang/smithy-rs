/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rulesengine.aws.language.functions.AwsBuiltIns
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameter
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.ConditionalDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointCustomization
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointTypesGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rustName
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docsOrFallback
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.orNull

/**
 * Client codegen decorator for the `AWS::Auth::AccountId` endpoint built-in parameter.
 *
 * The `AccountID` parameter is special because:
 * - The setters are neither exposed in `SdkConfig` nor in config builder.
 * - It does not require customizations like `loadBuiltInFromServiceConfig`,
 *  as it is not available when the `read_before_execution` method of the endpoint parameters interceptor is executed
 *  (the identity from which an account ID is retrieved has not been resolved yet).
 */
class AccountIdBuiltInParamDecorator : ConditionalDecorator(
    predicate =
        { codegenContext, _ ->
            codegenContext?.let {
                codegenContext.getBuiltIn(AwsBuiltIns.ACCOUNT_ID) != null
            } ?: false
        },
    delegateTo =
        object : ClientCodegenDecorator {
            override val name: String get() = "AccountIdBuiltInParamDecorator"
            override val order: Byte = 0

            override fun endpointCustomizations(codegenContext: ClientCodegenContext): List<EndpointCustomization> =
                listOf(
                    object : EndpointCustomization {
                        override fun serviceSpecificEndpointParamsFinalizer(
                            codegenContext: ClientCodegenContext,
                            params: String,
                        ): Writable? {
                            val runtimeConfig = codegenContext.runtimeConfig
                            return writable {
                                rustTemplate(
                                    """
                                    // This is required to satisfy the borrow checker. By obtaining an `Option<Identity>`,
                                    // `params` is no longer mutably borrowed in the match expression below.
                                    // Furthermore, by using `std::mem::replace` with an empty `Identity`, we avoid
                                    // leaving the sensitive `Identity` inside `params` within `EndpointResolverParams`.
                                    let identity = $params
                                        .get_property_mut::<#{Identity}>()
                                        .map(|id| {
                                            std::mem::replace(
                                                id,
                                                #{Identity}::new((), #{None}),
                                            )
                                        });
                                    match (
                                        $params.get_mut::<#{Params}>(),
                                        identity
                                            .as_ref()
                                            .and_then(|id| id.property::<#{AccountId}>()),
                                    ) {
                                        (#{Some}(concrete_params), #{Some}(account_id)) => {
                                            concrete_params.account_id = #{Some}(account_id.as_str().to_string());
                                        }
                                        (#{Some}(_), #{None}) => {
                                            // No account ID; nothing to do.
                                        }
                                        (#{None}, _) => {
                                            return #{Err}("service-specific endpoint params was not present".into());
                                        }
                                    }
                                    """,
                                    *preludeScope,
                                    "AccountId" to
                                        AwsRuntimeType.awsCredentialTypes(runtimeConfig)
                                            .resolve("attributes::AccountId"),
                                    "Identity" to
                                        RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                                            .resolve("client::identity::Identity"),
                                    "Params" to EndpointTypesGenerator.fromContext(codegenContext).paramsStruct(),
                                )
                            }
                        }
                    },
                )
        },
)

/**
 * Client codegen decorator for the `AWS::Auth::AccountIdEndpointMode` endpoint built-in parameter.
 *
 * The `AccountIdEndpointMode` parameter is special because:
 * - The corresponding Rust type is an enum containing valid values
 */
class AccountIdEndpointModeBuiltInParamDecorator : ConditionalDecorator(
    predicate =
        { codegenContext, _ ->
            codegenContext?.let {
                codegenContext.getBuiltIn(AwsBuiltIns.ACCOUNT_ID_ENDPOINT_MODE) != null
            } ?: false
        },
    delegateTo =
        object : ClientCodegenDecorator {
            override val name: String get() = "AccountIdEndpointModeBuiltInParamDecorator"
            override val order: Byte = 0
            private val paramName = AwsBuiltIns.ACCOUNT_ID_ENDPOINT_MODE.name.rustName()

            override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> =
                listOf(
                    adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
                        rust("${section.serviceConfigBuilder}.set_$paramName(${section.sdkConfig}.$paramName().cloned());")
                    },
                )

            override fun configCustomizations(
                codegenContext: ClientCodegenContext,
                baseCustomizations: List<ConfigCustomization>,
            ): List<ConfigCustomization> {
                return baseCustomizations +
                    object : ConfigCustomization() {
                        private val codegenScope =
                            arrayOf(
                                *preludeScope,
                                "AccountIdEndpointMode" to
                                    AwsRuntimeType.awsTypes(codegenContext.runtimeConfig)
                                        .resolve("endpoint_config::AccountIdEndpointMode"),
                            )

                        override fun section(section: ServiceConfig): Writable {
                            return when (section) {
                                ServiceConfig.BuilderImpl ->
                                    writable {
                                        val docs = AwsBuiltIns.ACCOUNT_ID_ENDPOINT_MODE.documentation.orNull()
                                        docsOrFallback(docs)
                                        rustTemplate(
                                            """
                                            pub fn $paramName(mut self, $paramName: #{AccountIdEndpointMode}) -> Self {
                                                self.set_$paramName(#{Some}($paramName));
                                                self
                                            }""",
                                            *codegenScope,
                                        )

                                        docsOrFallback(docs)
                                        rustTemplate(
                                            """
                                            pub fn set_$paramName(&mut self, $paramName: #{Option}<#{AccountIdEndpointMode}>) -> &mut Self {
                                                self.config.store_or_unset($paramName);
                                                self
                                            }
                                            """,
                                            *codegenScope,
                                        )
                                    }

                                is ServiceConfig.BuilderFromConfigBag ->
                                    writable {
                                        rustTemplate(
                                            """
                                            ${section.builder}.set_$paramName(${section.configBag}.load::<#{AccountIdEndpointMode}>().cloned());
                                            """,
                                            "AccountIdEndpointMode" to
                                                AwsRuntimeType.awsTypes(codegenContext.runtimeConfig)
                                                    .resolve("endpoint_config::AccountIdEndpointMode"),
                                        )
                                    }

                                else -> emptySection
                            }
                        }
                    }
            }

            override fun endpointCustomizations(codegenContext: ClientCodegenContext): List<EndpointCustomization> =
                listOf(
                    object : EndpointCustomization {
                        private val codegenScope =
                            arrayOf(
                                *preludeScope,
                                "AccountIdEndpointMode" to
                                    AwsRuntimeType.awsTypes(codegenContext.runtimeConfig)
                                        .resolve("endpoint_config::AccountIdEndpointMode"),
                                "AwsSdkFeature" to
                                    AwsRuntimeType.awsRuntime(codegenContext.runtimeConfig)
                                        .resolve("sdk_feature::AwsSdkFeature"),
                                "tracing" to RuntimeType.Tracing,
                            )

                        override fun loadBuiltInFromServiceConfig(
                            parameter: Parameter,
                            configRef: String,
                        ): Writable? =
                            when (parameter) {
                                AwsBuiltIns.ACCOUNT_ID_ENDPOINT_MODE -> {
                                    writable {
                                        rustTemplate(
                                            "#{Some}($configRef.load::<#{AccountIdEndpointMode}>().cloned().unwrap_or_default().to_string())",
                                            *codegenScope,
                                        )
                                    }
                                }

                                else -> null
                            }

                        // This override is for endpoint_tests.
                        override fun setBuiltInOnServiceConfig(
                            name: String,
                            value: Node,
                            configBuilderRef: String,
                        ): Writable? {
                            if (name != AwsBuiltIns.ACCOUNT_ID_ENDPOINT_MODE.builtIn.get()) {
                                return null
                            }
                            return writable {
                                rustTemplate(
                                    """let $configBuilderRef = $configBuilderRef.account_id_endpoint_mode(<#{AccountIdEndpointMode} as std::str::FromStr>::from_str(${value.expectStringNode().value.dq()}).expect("should parse"));""",
                                    *codegenScope,
                                )
                            }
                        }
                    },
                )

            override fun serviceRuntimePluginCustomizations(
                codegenContext: ClientCodegenContext,
                baseCustomizations: List<ServiceRuntimePluginCustomization>,
            ): List<ServiceRuntimePluginCustomization> =
                baseCustomizations + listOf(AccountIdEndpointFeatureTrackerInterceptor(codegenContext))
        },
)

private class AccountIdEndpointFeatureTrackerInterceptor(codegenContext: ClientCodegenContext) :
    ServiceRuntimePluginCustomization() {
    override fun section(section: ServiceRuntimePluginSection) =
        writable {
            if (section is ServiceRuntimePluginSection.RegisterRuntimeComponents) {
                section.registerInterceptor(this) {
                    rustTemplate(
                        "#{Interceptor}",
                        "Interceptor" to
                            RuntimeType.forInlineDependency(
                                InlineAwsDependency.forRustFile(
                                    "account_id_endpoint",
                                ),
                            ).resolve("AccountIdEndpointFeatureTrackerInterceptor"),
                    )
                }
            }
        }
}
