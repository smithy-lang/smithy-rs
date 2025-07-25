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
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
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

class EnvironmentTokenProviderDecorator(signingName: String) : ConditionalDecorator(
    predicate = { codegenContext, _ ->
        codegenContext?.serviceShape?.getTrait<SigV4Trait>()?.name == signingName
    },
    delegateTo =
        object : ClientCodegenDecorator {
            // private val  = signingName.replace("[-\\s]", "").toPascalCase()
            override val name = "${signingName.toPascalCase()}EnvironmentTokenProviderDecorator"

            // This decorator must decorate after `TokenProvidersDecorator` and  `AuthDecorator`
            // because those decorators render `builder.set_XXX` within `From<&SdkConfig> for Builder`,
            // which sets the field origin to be `Origin::service_config` because setters are called
            // explicitly on the builder.
            // This decorator needs to override those origins by those coming from `SdkConfig`
            override val order: Byte = (maxOf(TokenProvidersDecorator.ORDER, AuthDecorator.ORDER) + 1).toByte()

            override fun configCustomizations(
                codegenContext: ClientCodegenContext,
                baseCustomizations: List<ConfigCustomization>,
            ): List<ConfigCustomization> =
                baseCustomizations + EnvironmentTokenProviderConfigCustomization(codegenContext, signingName)

            override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> =
                listOf(
                    adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
                        section.inheritFieldOriginFromSdkConfig(this, "token_provider")
                        section.inheritFieldOriginFromSdkConfig(this, "auth_scheme_preference")
                    },
                )
        },
)

private class EnvironmentTokenProviderConfigCustomization(codegenContext: ClientCodegenContext, private val signingName: String) :
    ConfigCustomization() {
    private val rc = codegenContext.runtimeConfig
    private val codegenScope =
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

    override fun section(section: ServiceConfig) =
        when (section) {
            is ServiceConfig.BuilderBuild ->
                writable {
                    rustTemplate(
                        """
                        if let #{Some}(val) = #{LoadServiceConfig}::load_config(
                                &self.service_config_loader, service_config_key(
                                 ${signingName.dq()},
                                 "AWS_BEARER_TOKEN",
                                 "",
                             ))
                             .map(|it| it.parse::<#{String}>().unwrap())
                         {
                              if !self
                                  .config_origins
                                  .get("auth_scheme_preference")
                                  .map(|origin| origin.is_client_config())
                                  .unwrap_or_default()
                             {
                                 ${section.layer}.store_put(#{AuthSchemePreference}::from([#{HTTP_BEARER_AUTH_SCHEME_ID}]));
                             }
                             if !self
                                 .config_origins
                                 .get("token_provider")
                                 .map(|origin| origin.is_client_config())
                                 .unwrap_or_default() {
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
                }

            else -> emptySection
        }
}
