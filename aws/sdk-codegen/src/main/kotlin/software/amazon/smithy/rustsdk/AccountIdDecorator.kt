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
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.Types
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rustName
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docsOrFallback
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.orNull

/**
 * client codegen decorator for the `accountID` built-in parameter.
 *
 * The `accountID` parameter is special because:
 * - It is neither exposed in the `SdkConfig` setter nor in config builder.
 * - It does not require customizations like `loadBuiltInFromServiceConfig`,
 *  as it is not available when the `read_before_execution` method of the endpoint parameters interceptor is executed.
 */
class AccountIdDecorator : ConditionalDecorator(
    predicate =
        {
                codegenContext, _ ->
            codegenContext?.let {
                codegenContext.getBuiltIn(AwsBuiltIns.ACCOUNT_ID) != null
            } ?: false
        },
    delegateTo =
        object : ClientCodegenDecorator {
            override val name: String get() = "DecoratorForAccountId"
            override val order: Byte = 0

            override fun endpointCustomizations(codegenContext: ClientCodegenContext): List<EndpointCustomization> =
                listOf(
                    object : EndpointCustomization {
                        override fun overrideFinalizeEndpointParams(codegenContext: ClientCodegenContext): Writable? {
                            val runtimeConfig = codegenContext.runtimeConfig
                            return writable {
                                rustTemplate(
                                    """
                                    fn finalize_params<'a>(&'a self, params: &'a mut #{EndpointResolverParams}) -> #{FinalizeParamsFuture}<'a> {
                                        // This is required to satisfy the borrow checker. By obtaining an `Option<Identity>`,
                                        // `params` is no longer mutably borrowed in the match expression below.
                                        // Furthermore, by using `std::mem::replace` with an empty `Identity`, we avoid
                                        // leaving the sensitive `Identity` inside `params` within `EndpointResolverParams`.
                                        let identity = params
                                            .get_property_mut::<#{Identity}>()
                                            .map(|id| {
                                                std::mem::replace(
                                                    id,
                                                    #{Identity}::new((), #{None}),
                                                )
                                            });
                                        match (
                                            params.get_mut::<#{Params}>(),
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
                                                return #{FinalizeParamsFuture}::ready(
                                                    #{Err}("service-specific endpoint params was not present".into()),
                                                );
                                            }
                                        }
                                        #{FinalizeParamsFuture}::ready(#{Ok}(()))
                                    }
                                    """,
                                    *preludeScope,
                                    *Types(runtimeConfig).toArray(),
                                    "AccountId" to
                                        AwsRuntimeType.awsCredentialTypes(runtimeConfig)
                                            .resolve("attributes::AccountId"),
                                    "FinalizeParamsFuture" to
                                        RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                                            .resolve("client::endpoint::FinalizeParamsFuture"),
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

class AccountIdEndpointModeDecorator : ConditionalDecorator(
    predicate =
        { codegenContext, _ ->
            codegenContext?.let {
                codegenContext.getBuiltIn(AwsBuiltIns.ACCOUNT_ID_ENDPOINT_MODE) != null
            } ?: false
        },
    delegateTo =
        object : ClientCodegenDecorator {
            override val name: String get() = "DecoratorForAccountIdEndpointMode"
            override val order: Byte = 0

            override fun configCustomizations(
                codegenContext: ClientCodegenContext,
                baseCustomizations: List<ConfigCustomization>,
            ): List<ConfigCustomization> {
                val paramName = AwsBuiltIns.ACCOUNT_ID_ENDPOINT_MODE.name.rustName()
                return baseCustomizations +
                    object : ConfigCustomization() {
                        override fun section(section: ServiceConfig): Writable {
                            return when (section) {
                                ServiceConfig.BuilderImpl ->
                                    writable {
                                        docsOrFallback(AwsBuiltIns.ACCOUNT_ID_ENDPOINT_MODE.documentation.orNull())
                                        rustTemplate(
                                            """
                                            pub fn $paramName(mut self, $paramName: #{AccountIdEndpointMode}) -> Self {
                                                self.set_$paramName(#{Some}($paramName));
                                                self
                                            }""",
                                            *preludeScope,
                                            "AccountIdEndpointMode" to
                                                AwsRuntimeType.awsTypes(codegenContext.runtimeConfig)
                                                    .resolve("endpoint_config::AccountIdEndpointMode"),
                                        )

                                        docsOrFallback(AwsBuiltIns.ACCOUNT_ID_ENDPOINT_MODE.documentation.orNull())
                                        rustTemplate(
                                            """
                                            pub fn set_$paramName(&mut self, $paramName: #{Option}<#{AccountIdEndpointMode}>) -> &mut Self {
                                                self.config.store_or_unset($paramName);
                                                self
                                            }
                                            """,
                                            *preludeScope,
                                            "AccountIdEndpointMode" to
                                                AwsRuntimeType.awsTypes(codegenContext.runtimeConfig)
                                                    .resolve("endpoint_config::AccountIdEndpointMode"),
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
                        override fun loadBuiltInFromServiceConfig(
                            parameter: Parameter,
                            configRef: String,
                        ): Writable? =
                            when (parameter) {
                                AwsBuiltIns.ACCOUNT_ID_ENDPOINT_MODE -> {
                                    writable {
                                        rustTemplate(
                                            "$configRef.load::<#{AccountIdEndpointMode}>().map(|m| m.to_string())",
                                            "AccountIdEndpointMode" to
                                                AwsRuntimeType.awsTypes(codegenContext.runtimeConfig)
                                                    .resolve("endpoint_config::AccountIdEndpointMode"),
                                        )
                                    }
                                }

                                else -> null
                            }

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
                                    "AccountIdEndpointMode" to
                                        AwsRuntimeType.awsTypes(codegenContext.runtimeConfig)
                                            .resolve("endpoint_config::AccountIdEndpointMode"),
                                )
                            }
                        }
                    },
                )
        },
)
