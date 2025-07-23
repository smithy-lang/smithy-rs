/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize

import software.amazon.smithy.aws.traits.auth.SigV4Trait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.auth.AuthDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.ConditionalDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rustsdk.AwsRuntimeType
import software.amazon.smithy.rustsdk.SdkConfigSection
import software.amazon.smithy.rustsdk.TokenProvidersDecorator

class EnvironmentTokenProviderDecorator(signingName: String) : ConditionalDecorator(
    predicate = { codegenContext, _ ->
        codegenContext?.serviceShape?.getTrait<SigV4Trait>()?.name == signingName
    },
    delegateTo =
        object : ClientCodegenDecorator {
            private val pascalCasedSigningName = signingName.replace("[-\\s]", "").toPascalCase()
            override val name = "${pascalCasedSigningName}EnvironmentTokenProviderDecorator"

            // This decorator must decorate after `TokenProvidersDecorator` and  `AuthDecorator`
            override val order: Byte = (maxOf(TokenProvidersDecorator.ORDER, AuthDecorator.ORDER) + 1).toByte()

            override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> =
                listOf(
                    adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { _ ->
                        val runtimeConfig = codegenContext.runtimeConfig
                        rustTemplate(
                            """
                            if let Some(val) = input.service_config().and_then(|conf| {
                                conf.load_config(service_config_key(#{Some}(${signingName.dq()}), "AWS_BEARER_TOKEN", ""))
                                    .map(|it| it.parse::<#{String}>().unwrap())
                            }) {
                                if !input.get_origin("auth_scheme_preference").is_client_config() {
                                    builder.set_auth_scheme_preference(Some([#{HTTP_BEARER_AUTH_SCHEME_ID}].into()));
                                }
                                if !input.get_origin("token_provider").is_client_config() {
                                    let mut layer = #{Layer}::new("AwsBearerToken$pascalCasedSigningName");
                                    layer.store_append(
                                        #{AwsSdkFeature}::BearerServiceEnvVars
                                    );
                                    let identity = #{Identity}::builder()
                                        .data(#{Token}::new(val, #{None}))
                                        .property(layer.freeze())
                                        .build()
                                        .unwrap();
                                    builder.runtime_components.set_identity_resolver(
                                        #{HTTP_BEARER_AUTH_SCHEME_ID},
                                        identity,
                                    );
                                }
                            }
                            """,
                            *preludeScope,
                            "AwsSdkFeature" to
                                AwsRuntimeType.awsRuntime(runtimeConfig)
                                    .resolve("sdk_feature::AwsSdkFeature"),
                            "HTTP_BEARER_AUTH_SCHEME_ID" to
                                CargoDependency.smithyRuntimeApiClient(runtimeConfig)
                                    .withFeature("http-auth").toType().resolve("client::auth::http")
                                    .resolve("HTTP_BEARER_AUTH_SCHEME_ID"),
                            "Identity" to
                                RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                                    .resolve("client::identity::Identity"),
                            "Layer" to RuntimeType.smithyTypes(runtimeConfig).resolve("config_bag::Layer"),
                            "Token" to AwsRuntimeType.awsCredentialTypes(runtimeConfig).resolve("Token"),
                        )
                    },
                )
        },
)
