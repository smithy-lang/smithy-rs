/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.TestUtilFeature
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization
import software.amazon.smithy.rust.codegen.core.util.letIf

class CredentialsProviderDecorator : ClientCodegenDecorator {
    override val name: String = "CredentialsProvider"
    override val order: Byte = 0

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> =
        baseCustomizations.letIf(codegenContext.smithyRuntimeMode.generateOrchestrator) {
            it + listOf(CredentialsIdentityResolverRegistration(codegenContext))
        }

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations + CredentialProviderConfig(codegenContext.runtimeConfig)
    }

    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> =
        listOf(
            adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
                rust("${section.serviceConfigBuilder}.set_credentials_provider(${section.sdkConfig}.credentials_provider());")
            },
        )

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        rustCrate.mergeFeature(TestUtilFeature.copy(deps = listOf("aws-credential-types/test-util")))

        rustCrate.withModule(ClientRustModule.Config) {
            rust(
                "pub use #T::Credentials;",
                AwsRuntimeType.awsCredentialTypes(codegenContext.runtimeConfig),
            )
        }
    }
}

/**
 * Add a `.credentials_provider` field and builder to the `Config` for a given service
 */
class CredentialProviderConfig(runtimeConfig: RuntimeConfig) : ConfigCustomization() {
    private val codegenScope = arrayOf(
        "provider" to AwsRuntimeType.awsCredentialTypes(runtimeConfig).resolve("provider"),
        "Credentials" to AwsRuntimeType.awsCredentialTypes(runtimeConfig).resolve("Credentials"),
        "TestCredentials" to AwsRuntimeType.awsCredentialTypesTestUtil(runtimeConfig).resolve("Credentials"),
    )

    override fun section(section: ServiceConfig) = writable {
        when (section) {
            ServiceConfig.BuilderStruct ->
                rustTemplate("credentials_provider: Option<#{provider}::SharedCredentialsProvider>,", *codegenScope)
            ServiceConfig.BuilderImpl -> {
                rustTemplate(
                    """
                    /// Sets the credentials provider for this service
                    pub fn credentials_provider(mut self, credentials_provider: impl #{provider}::ProvideCredentials + 'static) -> Self {
                        self.set_credentials_provider(Some(#{provider}::SharedCredentialsProvider::new(credentials_provider)));
                        self
                    }

                    /// Sets the credentials provider for this service
                    pub fn set_credentials_provider(&mut self, credentials_provider: Option<#{provider}::SharedCredentialsProvider>) -> &mut Self {
                        self.credentials_provider = credentials_provider;
                        self
                    }
                    """,
                    *codegenScope,
                )
            }

            is ServiceConfig.DefaultForTests -> rustTemplate(
                "${section.configBuilderRef}.set_credentials_provider(Some(#{provider}::SharedCredentialsProvider::new(#{TestCredentials}::for_tests())));",
                *codegenScope,
            )

            else -> emptySection
        }
    }
}

class CredentialsIdentityResolverRegistration(
    codegenContext: ClientCodegenContext,
) : ServiceRuntimePluginCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig

    override fun section(section: ServiceRuntimePluginSection): Writable = writable {
        when (section) {
            is ServiceRuntimePluginSection.AdditionalConfig -> {
                rustTemplate(
                    """
                    cfg.set_identity_resolvers(
                        #{IdentityResolvers}::builder()
                            .identity_resolver(
                                #{SIGV4_SCHEME_ID},
                                #{CredentialsIdentityResolver}::new(self.handle.conf.credentials_cache())
                            )
                            .build()
                    );
                    """,
                    "SIGV4_SCHEME_ID" to AwsRuntimeType.awsRuntime(runtimeConfig)
                        .resolve("auth::sigv4::SCHEME_ID"),
                    "CredentialsIdentityResolver" to AwsRuntimeType.awsRuntime(runtimeConfig)
                        .resolve("identity::credentials::CredentialsIdentityResolver"),
                    "IdentityResolvers" to RuntimeType.smithyRuntimeApi(runtimeConfig)
                        .resolve("client::identity::IdentityResolvers"),
                )
            }
            else -> {}
        }
    }
}

fun defaultProvider() =
    RuntimeType.forInlineDependency(InlineAwsDependency.forRustFile("no_credentials")).resolve("NoCredentials")
