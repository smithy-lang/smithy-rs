/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize

import software.amazon.smithy.aws.traits.auth.SigV4Trait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
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
import software.amazon.smithy.rustsdk.ServiceConfigSection

class EnvironmentTokenProviderDecorator(signingName: String) : ConditionalDecorator(
    predicate = { codegenContext, _ ->
        codegenContext?.serviceShape?.getTrait<SigV4Trait>()?.name == signingName
    },
    delegateTo =
        object : ClientCodegenDecorator {
            // private val  = signingName.replace("[-\\s]", "").toPascalCase()
            override val name = "${signingName.toPascalCase()}EnvironmentTokenProviderDecorator"
            override val order: Byte = 0

            override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> {
                val rc = codegenContext.runtimeConfig
                val codegenScope =
                    arrayOf(
                        *preludeScope,
                        "AuthSchemePreference" to
                            RuntimeType.smithyRuntimeApiClient(rc)
                                .resolve("client::auth::AuthSchemePreference"),
                        "AwsSdkFeature" to
                            AwsRuntimeType.awsRuntime(rc)
                                .resolve("sdk_feature::AwsSdkFeature"),
                        "Env" to AwsRuntimeType.awsTypes(rc).resolve("os_shim_internal::Env"),
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
                    adhocCustomization<ServiceConfigSection.LoadFromServiceSpecificEnv> { _ ->
                        rustTemplate(
                            """
                            if let #{Some}(val) =
                                #{LoadServiceConfig}::load_config(
                                    &env_config_loader, service_config_key(
                                    ${signingName.dq()},
                                    "AWS_BEARER_TOKEN",
                                    "",
                                ))
                                .map(|it| it.parse::<#{String}>().unwrap())
                            {
                                if self.field_never_set::<#{AuthSchemePreference}>() &&
                                    !self.explicitly_set_in_shared_config("aws_scheme_preference")
                                {
                                    self.set_auth_scheme_preference(#{Some}(#{AuthSchemePreference}::from([#{HTTP_BEARER_AUTH_SCHEME_ID}])));
                                }

                                if self
                                    .runtime_components
                                    .identity_resolver(
                                        &::aws_smithy_runtime_api::client::auth::http::HTTP_BEARER_AUTH_SCHEME_ID,
                                    )
                                    .is_none()
                                    && !self.explicitly_set_in_shared_config("token_provider")
                                {
                                    let mut layer = #{Layer}::new("AwsBearerToken${signingName.toPascalCase()}");
                                    layer.store_append(#{AwsSdkFeature}::BearerServiceEnvVars);
                                    let identity = #{Identity}::builder()
                                        .data(#{Token}::new(val, #{None}))
                                        .property(layer.freeze())
                                        .build()
                                        .unwrap();
                                    self.runtime_components.set_identity_resolver(#{HTTP_BEARER_AUTH_SCHEME_ID}, identity);
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
