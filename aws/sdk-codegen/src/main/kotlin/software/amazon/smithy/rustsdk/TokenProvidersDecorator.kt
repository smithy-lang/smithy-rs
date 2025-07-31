/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.knowledge.ServiceIndex
import software.amazon.smithy.model.traits.HttpBearerAuthTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.configReexport
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.ConditionalDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization

class TokenProvidersDecorator : ConditionalDecorator(
    predicate = { codegenContext, _ ->
        codegenContext?.let {
            ServiceIndex.of(codegenContext.model).getEffectiveAuthSchemes(codegenContext.serviceShape)
                .containsKey(HttpBearerAuthTrait.ID)
        } ?: false
    },
    delegateTo =
        object : ClientCodegenDecorator {
            override val name: String get() = "TokenProvidersDecorator"
            override val order: Byte = ORDER

            override fun configCustomizations(
                codegenContext: ClientCodegenContext,
                baseCustomizations: List<ConfigCustomization>,
            ): List<ConfigCustomization> = baseCustomizations + TokenProviderConfig(codegenContext)

            override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> =
                listOf(
                    adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
                        rust("${section.serviceConfigBuilder}.set_token_provider(${section.sdkConfig}.token_provider());")
                    },
                )
        },
) {
    companion object {
        const val ORDER: Byte = 0
    }
}

/**
 * Add a `.token_provider` field and builder to the `Config` for a given service
 */
class TokenProviderConfig(private val codegenContext: ClientCodegenContext) : ConfigCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            *RuntimeType.preludeScope,
            "IntoShared" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("shared::IntoShared"),
            "Token" to configReexport(AwsRuntimeType.awsCredentialTypes(runtimeConfig).resolve("Token")),
            "ProvideToken" to
                configReexport(
                    AwsRuntimeType.awsCredentialTypes(runtimeConfig)
                        .resolve("provider::token::ProvideToken"),
                ),
            "SharedTokenProvider" to
                configReexport(
                    AwsRuntimeType.awsCredentialTypes(runtimeConfig)
                        .resolve("provider::token::SharedTokenProvider"),
                ),
            "TestToken" to AwsRuntimeType.awsCredentialTypesTestUtil(runtimeConfig).resolve("Token"),
            "HTTP_BEARER_AUTH_SCHEME_ID" to
                CargoDependency.smithyRuntimeApiClient(runtimeConfig)
                    .withFeature("http-auth").toType().resolve("client::auth::http")
                    .resolve("HTTP_BEARER_AUTH_SCHEME_ID"),
        )

    override fun section(section: ServiceConfig) =
        writable {
            when (section) {
                ServiceConfig.BuilderImpl -> {
                    rustTemplate(
                        """
                        /// Sets the access token provider for this service
                        ///
                        /// Note: the [`Self::bearer_token`] and [`Self::bearer_token_resolver`] methods are
                        /// equivalent to this method, but take the [`Token`] and [`ResolveIdentity`] types
                        /// respectively.
                        ///
                        /// [`Token`]: crate::config::Token
                        /// [`ResolveIdentity`]: crate::config::ResolveIdentity
                        pub fn token_provider(mut self, token_provider: impl #{ProvideToken} + 'static) -> Self {
                            self.set_token_provider(#{Some}(#{IntoShared}::<#{SharedTokenProvider}>::into_shared(token_provider)));
                            self
                        }

                        /// Sets the access token provider for this service
                        ///
                        /// Note: the [`Self::bearer_token`] and [`Self::bearer_token_resolver`] methods are
                        /// equivalent to this method, but take the [`Token`] and [`ResolveIdentity`] types
                        /// respectively.
                        ///
                        /// [`Token`]: crate::config::Token
                        /// [`ResolveIdentity`]: crate::config::ResolveIdentity
                        pub fn set_token_provider(&mut self, token_provider: #{Option}<#{SharedTokenProvider}>) -> &mut Self {
                            if let Some(token_provider) = token_provider {
                                self.runtime_components.set_identity_resolver(#{HTTP_BEARER_AUTH_SCHEME_ID}, token_provider);
                            }
                            self
                        }
                        """,
                        *codegenScope,
                    )
                }

                is ServiceConfig.DefaultForTests ->
                    rustTemplate(
                        "${section.configBuilderRef}.set_token_provider(Some(#{SharedTokenProvider}::new(#{TestToken}::for_tests())));",
                        *codegenScope,
                    )

                else -> emptySection
            }
        }
}
