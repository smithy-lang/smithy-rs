/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize

import software.amazon.smithy.aws.traits.auth.SigV4Trait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.auth.AuthDecorator
import software.amazon.smithy.rust.codegen.client.smithy.configReexport
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

/** Environment variable prefix for AWS bearer tokens */
const val AWS_BEARER_TOKEN = "AWS_BEARER_TOKEN"

/**
 * A code generation decorator that adds support for service-specific environment variable-based
 * bearer token authentication.
 *
 * This decorator is relevant to AWS services whose SigV4 service signing name matches the input
 * to the decorator. It generates code that allows these services to automatically configure bearer
 * token authentication from service-specific environment variables.
 *
 * @param signingName The AWS service signing name used to match against SigV4 traits and construct
 *                    the environment variable name (e.g., "bedrock" -> AWS_BEARER_TOKEN_BEDROCK)
 */
class EnvironmentTokenProviderDecorator(signingName: String) : ConditionalDecorator(
    predicate = { codegenContext, _ ->
        codegenContext?.serviceShape?.getTrait<SigV4Trait>()?.name == signingName
    },
    delegateTo =
        object : ClientCodegenDecorator {
            override val name = "${signingName.toPascalCase()}EnvironmentTokenProviderDecorator"

            // This decorator must decorate after `TokenProvidersDecorator` and `AuthDecorator`,
            // so it injects code within `impl From<&SdkConfig> for Builder` like so
            // ```
            // impl From<&SdkConfig> for Builder {
            //     fn from(input: &SdkConfig) -> Self {
            //         let mut builder = Builder::default();
            //         // --snip--
            //         builder.set_auth_scheme_preference(input.auth_scheme_preference()); // by AuthDecorator
            //         builder.set_token_provider(input.token_provider()); // by TokenProvidersDecorator
            //         // --snip--
            //         // auth scheme preference and token provider from service specific env
            //         // should be set after the lines above to avoid incorrectly being overwritten
            //     }
            // }
            // ```
            override val order: Byte = (maxOf(TokenProvidersDecorator.ORDER, AuthDecorator.ORDER) + 1).toByte()

            override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> {
                val rc = codegenContext.runtimeConfig
                val codegenScope =
                    arrayOf(
                        *preludeScope,
                        "AwsCredentialFeature" to
                            AwsRuntimeType.awsCredentialTypes(rc)
                                .resolve("credential_feature::AwsCredentialFeature"),
                        "HTTP_BEARER_AUTH_SCHEME_ID" to
                            CargoDependency.smithyRuntimeApiClient(rc)
                                .withFeature("http-auth").toType().resolve("client::auth::http")
                                .resolve("HTTP_BEARER_AUTH_SCHEME_ID"),
                        "Identity" to RuntimeType.smithyRuntimeApiClient(rc).resolve("client::identity::Identity"),
                        "Layer" to RuntimeType.smithyTypes(rc).resolve("config_bag::Layer"),
                        "LoadServiceConfig" to AwsRuntimeType.awsTypes(rc).resolve("service_config::LoadServiceConfig"),
                        "Token" to configReexport(AwsRuntimeType.awsCredentialTypes(rc).resolve("Token")),
                    )

                return listOf(
                    adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { _ ->
                        rustTemplate(
                            """
                            if let #{Some}(val) = input.service_config().and_then(|conf| {
                                // Passing an empty string for the last argument of `service_config_key`,
                                // since shared config/profile for environment token provider is not supported.
                                #{LoadServiceConfig}::load_config(
                                    conf,
                                    service_config_key(${signingName.replace("-", " ").dq()}, ${AWS_BEARER_TOKEN.dq()}, "")
                                )
                                .and_then(|it| it.parse::<#{String}>().ok())
                            }) {
                                if !input.get_origin("auth_scheme_preference").is_client_config() {
                                    builder.set_auth_scheme_preference(#{Some}([#{HTTP_BEARER_AUTH_SCHEME_ID}].into()));
                                }
                                if !input.get_origin("token_provider").is_client_config() {
                                    let mut layer = #{Layer}::new("AwsBearerToken${signingName.toPascalCase()}");
                                    layer.store_append(#{AwsCredentialFeature}::BearerServiceEnvVars);
                                    let identity = #{Identity}::builder()
                                        .data(#{Token}::new(val, #{None}))
                                        .property(layer.freeze())
                                        .build()
                                        .expect("required fields set");
                                    builder.runtime_components.set_identity_resolver(#{HTTP_BEARER_AUTH_SCHEME_ID}, identity);
                                }
                            }
                            """,
                            *codegenScope,
                        )
                    },
                )
            }
        },
)
