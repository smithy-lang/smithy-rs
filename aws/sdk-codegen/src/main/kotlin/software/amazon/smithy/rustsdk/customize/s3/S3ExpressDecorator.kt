/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.s3

import software.amazon.smithy.aws.traits.auth.SigV4Trait
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.configReexport
import software.amazon.smithy.rust.codegen.client.smithy.customize.AuthSchemeOption
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rustsdk.AwsRuntimeType
import software.amazon.smithy.rustsdk.InlineAwsDependency

class S3ExpressDecorator : ClientCodegenDecorator {
    override val name: String = "S3ExpressDecorator"
    override val order: Byte = 0

    private fun sigv4S3Express() =
        writable {
            rust(
                "#T",
                RuntimeType.forInlineDependency(
                    InlineAwsDependency.forRustFile("s3_express"),
                ).resolve("auth::SCHEME_ID"),
            )
        }

    override fun authOptions(
        codegenContext: ClientCodegenContext,
        operationShape: OperationShape,
        baseAuthSchemeOptions: List<AuthSchemeOption>,
    ): List<AuthSchemeOption> =
        baseAuthSchemeOptions +
            AuthSchemeOption.StaticAuthSchemeOption(
                SigV4Trait.ID,
                listOf(sigv4S3Express()),
            )

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> =
        baseCustomizations + listOf(S3ExpressServiceRuntimePluginCustomization(codegenContext))

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> = baseCustomizations + listOf(S3ExpressIdentityProviderConfig(codegenContext))
}

private class S3ExpressServiceRuntimePluginCustomization(codegenContext: ClientCodegenContext) :
    ServiceRuntimePluginCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope by lazy {
        arrayOf(
            "DefaultS3ExpressIdentityProvider" to
                RuntimeType.forInlineDependency(
                    InlineAwsDependency.forRustFile("s3_express"),
                ).resolve("identity_provider::DefaultS3ExpressIdentityProvider"),
            "IdentityCacheLocation" to
                RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                    .resolve("client::identity::IdentityCacheLocation"),
            "S3ExpressAuthScheme" to
                RuntimeType.forInlineDependency(
                    InlineAwsDependency.forRustFile("s3_express"),
                ).resolve("auth::S3ExpressAuthScheme"),
            "S3_EXPRESS_SCHEME_ID" to
                RuntimeType.forInlineDependency(
                    InlineAwsDependency.forRustFile("s3_express"),
                ).resolve("auth::SCHEME_ID"),
            "SharedAuthScheme" to
                RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                    .resolve("client::auth::SharedAuthScheme"),
            "SharedIdentityResolver" to
                RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                    .resolve("client::identity::SharedIdentityResolver"),
        )
    }

    override fun section(section: ServiceRuntimePluginSection): Writable =
        writable {
            when (section) {
                is ServiceRuntimePluginSection.RegisterRuntimeComponents -> {
                    section.registerAuthScheme(this) {
                        rustTemplate(
                            "#{SharedAuthScheme}::new(#{S3ExpressAuthScheme}::new())",
                            *codegenScope,
                        )
                    }

                    section.registerIdentityResolver(
                        this,
                        writable {
                            rustTemplate("#{S3_EXPRESS_SCHEME_ID}", *codegenScope)
                        },
                        writable {
                            rustTemplate("#{DefaultS3ExpressIdentityProvider}::builder().build()", *codegenScope)
                        },
                    )
                }

                else -> {}
            }
        }
}

class S3ExpressIdentityProviderConfig(codegenContext: ClientCodegenContext) : ConfigCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "IdentityCacheLocation" to
                RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                    .resolve("client::identity::IdentityCacheLocation"),
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
            "SharedIdentityResolver" to
                RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                    .resolve("client::identity::SharedIdentityResolver"),
            "S3_EXPRESS_SCHEME_ID" to
                RuntimeType.forInlineDependency(
                    InlineAwsDependency.forRustFile("s3_express"),
                ).resolve("auth::SCHEME_ID"),
        )

    override fun section(section: ServiceConfig) =
        writable {
            when (section) {
                ServiceConfig.BuilderImpl -> {
                    rustTemplate(
                        """
                        /// Sets the credentials provider for S3 Express One Zone
                        pub fn express_credentials_provider(mut self, credentials_provider: impl #{ProvideCredentials} + 'static) -> Self {
                            self.set_express_credentials_provider(#{Some}(#{SharedCredentialsProvider}::new(credentials_provider)));
                            self
                        }
                        """,
                        *codegenScope,
                    )

                    rustTemplate(
                        """
                        /// Sets the credentials provider for S3 Express One Zone
                        pub fn set_express_credentials_provider(&mut self, credentials_provider: #{Option}<#{SharedCredentialsProvider}>) -> &mut Self {
                            if let #{Some}(credentials_provider) = credentials_provider {
                                self.runtime_components.set_identity_resolver(#{S3_EXPRESS_SCHEME_ID}, credentials_provider);
                            }
                            self
                        }
                        """,
                        *codegenScope,
                    )
                }

                else -> emptySection
            }
        }
}
