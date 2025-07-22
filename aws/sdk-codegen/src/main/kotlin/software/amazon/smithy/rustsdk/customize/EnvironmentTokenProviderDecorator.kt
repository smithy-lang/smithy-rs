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
import software.amazon.smithy.rust.codegen.client.smithy.customize.TestUtilFeature
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
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

const val AWS_BEARER_TOKEN = "AWS_BEARER_TOKEN"

class EnvironmentTokenProviderDecorator(signingName: String) : ConditionalDecorator(
    predicate = { codegenContext, _ ->
        codegenContext?.serviceShape?.getTrait<SigV4Trait>()?.name == signingName
    },
    delegateTo =
        object : ClientCodegenDecorator {
            override val name = "${signingName.replace("[-\\s]", "").toPascalCase()}EnvironmentTokenProviderDecorator"

            // This decorator must decorate after `TokenProvidersDecorator` and  `AuthDecorator`
            override val order: Byte = (maxOf(TokenProvidersDecorator.ORDER, AuthDecorator.ORDER) + 1).toByte()

            override fun configCustomizations(
                codegenContext: ClientCodegenContext,
                baseCustomizations: List<ConfigCustomization>,
            ): List<ConfigCustomization> =
                baseCustomizations + EnvironmentTokenProviderConfigCustomization(codegenContext, signingName)

            override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> =
                listOf(
                    adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { _ ->
                        rustTemplate(
                            """
                            if let Some(val) = input.service_config().and_then(|conf| {
                                conf.load_config(service_config_key(#{Some}(${signingName.dq()}), ${AWS_BEARER_TOKEN.dq()}, ""))
                                    .map(|it| it.parse::<#{String}>().unwrap())
                            }) {
                                if !input.get_origin("auth_scheme_preference").is_client_config() {
                                    builder.set_auth_scheme_preference(Some([#{HTTP_BEARER_AUTH_SCHEME_ID}].into()));
                                }
                                if !input.get_origin("token_provider").is_client_config() {
                                    builder = builder.bearer_token(#{Token}::new(val, #{None}));
                                }
                            }
                            """,
                            *preludeScope,
                            "HTTP_BEARER_AUTH_SCHEME_ID" to
                                CargoDependency.smithyRuntimeApiClient(codegenContext.runtimeConfig)
                                    .withFeature("http-auth").toType().resolve("client::auth::http")
                                    .resolve("HTTP_BEARER_AUTH_SCHEME_ID"),
                            "Token" to AwsRuntimeType.awsCredentialTypes(codegenContext.runtimeConfig).resolve("Token"),
                        )
                    },
                )
        },
)

private class EnvironmentTokenProviderConfigCustomization(codegenContext: ClientCodegenContext, signingName: String) : ConfigCustomization() {
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "AuthSchemePreference" to
                RuntimeType.smithyRuntimeApiClient(codegenContext.runtimeConfig)
                    .resolve("client::auth::AuthSchemePreference"),
            "Env" to AwsRuntimeType.awsTypes(codegenContext.runtimeConfig).resolve("os_shim_internal::Env"),
            "HTTP_BEARER_AUTH_SCHEME_ID" to
                CargoDependency.smithyRuntimeApiClient(codegenContext.runtimeConfig)
                    .withFeature("http-auth").toType().resolve("client::auth::http")
                    .resolve("HTTP_BEARER_AUTH_SCHEME_ID"),
            "Token" to configReexport(AwsRuntimeType.awsCredentialTypes(codegenContext.runtimeConfig).resolve("Token")),
        )

    // This should match the environment variable name generated by `aws_runtime::env_config::get_service_config_from_env`.
    private val environmentVariable = "${AWS_BEARER_TOKEN}_${signingName.replace("[-\\s]", "_").uppercase()}"

    override fun section(section: ServiceConfig) =
        writable {
            when (section) {
                ServiceConfig.BuilderStruct -> {
                    rustTemplate("env: #{Env}", *codegenScope)
                }

                ServiceConfig.BuilderImplDefaultFieldInit -> {
                    rustTemplate("env: #{Default}::default()", *codegenScope)
                }

                ServiceConfig.BuilderBuild -> {
                    rustTemplate(
                        """
                        if let Ok(val) = self.env.get(${environmentVariable.dq()}) {
                             if layer.load::<#{AuthSchemePreference}>().is_none() {
                                 layer.store_put(#{AuthSchemePreference}::from([#{HTTP_BEARER_AUTH_SCHEME_ID}]));
                             }
                             if self.runtime_components.identity_resolver(&#{HTTP_BEARER_AUTH_SCHEME_ID})
                                 .is_none()
                             {
                                 self.runtime_components.set_identity_resolver(
                                     #{HTTP_BEARER_AUTH_SCHEME_ID},
                                     #{Token}::new(val, None),
                                 );
                             }
                         }
                        """,
                        *codegenScope,
                    )
                }

                is ServiceConfig.DefaultForTests -> {
                    rustTemplate("self.env = #{Env}::from_slice(&[]);", *codegenScope)
                }

                is ServiceConfig.Extras -> {
                    val testUtilOnly =
                        Attribute(Attribute.cfg(Attribute.any(Attribute.feature(TestUtilFeature.name), writable("test"))))
                    testUtilOnly.render(this)
                    Attribute.DocHidden.render(this)
                    rustBlock("pub fn with_env<'a>(mut self, vars: &[(&'a str, &'a str)]) -> Self") {
                        rustTemplate(
                            """
                            self.env = #{Env}::from_slice(vars);
                            self
                            """,
                            *codegenScope,
                        )
                    }
                }

                else -> emptySection
            }
        }
}
