/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.configReexport
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.ConditionalDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.TestUtilFeature
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.featureGateBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization

class CredentialsProviderDecorator : ConditionalDecorator(
    predicate = { codegenContext, _ -> codegenContext?.usesSigAuth() ?: false },
    delegateTo =
        object : ClientCodegenDecorator {
            override val name: String = "CredentialsProviderDecorator"
            override val order: Byte = 0

            override fun configCustomizations(
                codegenContext: ClientCodegenContext,
                baseCustomizations: List<ConfigCustomization>,
            ): List<ConfigCustomization> = baseCustomizations + CredentialProviderConfig(codegenContext)

            override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> =
                listOf(
                    adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
                        rust("${section.serviceConfigBuilder}.set_credentials_provider(${section.sdkConfig}.credentials_provider());")
                        rustTemplate(
                            """
                            if ${section.sdkConfig}.credentials_provider().is_none() {
                                ${section.serviceConfigBuilder}.set_auth_scheme_resolver(#{no_auth_scheme_resolver}());
                            }
                            """,
                            "no_auth_scheme_resolver" to
                                ClientRustModule.Config.auth.toType()
                                    .resolve("no_auth_scheme_resolver"),
                        )
                    },
                )

            override fun extras(
                codegenContext: ClientCodegenContext,
                rustCrate: RustCrate,
            ) {
                rustCrate.mergeFeature(TestUtilFeature.copy(deps = listOf("aws-credential-types/test-util")))

                rustCrate.withModule(ClientRustModule.config) {
                    rust(
                        "pub use #T::Credentials;",
                        AwsRuntimeType.awsCredentialTypes(codegenContext.runtimeConfig),
                    )
                }
            }
        },
)

/**
 * Add a `.credentials_provider` field and builder to the `Config` for a given service
 */
class CredentialProviderConfig(private val codegenContext: ClientCodegenContext) : ConfigCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "Credentials" to configReexport(AwsRuntimeType.awsCredentialTypes(runtimeConfig).resolve("Credentials")),
            "ProvideCredentials" to
                configReexport(
                    AwsRuntimeType.awsCredentialTypes(runtimeConfig)
                        .resolve("provider::ProvideCredentials"),
                ),
            "SharedCredentialsProvider" to
                configReexport(
                    AwsRuntimeType.awsCredentialTypes(runtimeConfig)
                        .resolve("provider::SharedCredentialsProvider"),
                ),
            "SIGV4A_SCHEME_ID" to
                AwsRuntimeType.awsRuntime(runtimeConfig)
                    .resolve("auth::sigv4a::SCHEME_ID"),
            "SIGV4_SCHEME_ID" to
                AwsRuntimeType.awsRuntime(runtimeConfig)
                    .resolve("auth::sigv4::SCHEME_ID"),
            "TestCredentials" to AwsRuntimeType.awsCredentialTypesTestUtil(runtimeConfig).resolve("Credentials"),
        )

    override fun section(section: ServiceConfig) =
        writable {
            when (section) {
                ServiceConfig.ConfigImpl -> {
                    rustTemplate(
                        """
                        /// This function was intended to be removed, and has been broken since release-2023-11-15 as it always returns a `None`. Do not use.
                        ##[deprecated(note = "This function was intended to be removed, and has been broken since release-2023-11-15 as it always returns a `None`. Do not use.")]
                        pub fn credentials_provider(&self) -> Option<#{SharedCredentialsProvider}> {
                            #{None}
                        }
                        """,
                        *codegenScope,
                    )
                }

                ServiceConfig.BuilderImpl -> {
                    rustTemplate(
                        """
                        /// Sets the credentials provider for this service
                        pub fn credentials_provider(mut self, credentials_provider: impl #{ProvideCredentials} + 'static) -> Self {
                            self.set_credentials_provider(#{Some}(#{SharedCredentialsProvider}::new(credentials_provider)));
                            self
                        }
                        """,
                        *codegenScope,
                    )

                    rustBlockTemplate(
                        """
                        /// Sets the credentials provider for this service
                        pub fn set_credentials_provider(&mut self, credentials_provider: #{Option}<#{SharedCredentialsProvider}>) -> &mut Self
                        """,
                        *codegenScope,
                    ) {
                        rustBlockTemplate(
                            """
                            if let Some(credentials_provider) = credentials_provider
                            """,
                            *codegenScope,
                        ) {
                            if (codegenContext.usesSigV4a()) {
                                featureGateBlock("sigv4a") {
                                    rustTemplate(
                                        "self.runtime_components.set_identity_resolver(#{SIGV4A_SCHEME_ID}, credentials_provider.clone());",
                                        *codegenScope,
                                    )
                                }
                            }
                            rustTemplate(
                                "self.runtime_components.set_identity_resolver(#{SIGV4_SCHEME_ID}, credentials_provider);",
                                *codegenScope,
                            )
                        }
                        rust("self")
                    }
                }

                is ServiceConfig.DefaultForTests ->
                    rustTemplate(
                        "${section.configBuilderRef}.set_credentials_provider(Some(#{SharedCredentialsProvider}::new(#{TestCredentials}::for_tests())));",
                        *codegenScope,
                    )

                else -> emptySection
            }
        }
}
