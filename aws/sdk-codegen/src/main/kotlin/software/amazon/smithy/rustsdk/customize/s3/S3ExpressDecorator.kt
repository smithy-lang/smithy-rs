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
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization
import software.amazon.smithy.rustsdk.AwsCargoDependency
import software.amazon.smithy.rustsdk.AwsRuntimeType
import software.amazon.smithy.rustsdk.InlineAwsDependency
import software.amazon.smithy.rustsdk.SdkConfigSection

class S3ExpressDecorator : ClientCodegenDecorator {
    override val name: String = "S3ExpressDecorator"
    override val order: Byte = 0

    private fun sigv4S3Express(runtimeConfig: RuntimeConfig) =
        writable {
            rust(
                "#T",
                s3ExpressModule(runtimeConfig).resolve("auth::SCHEME_ID"),
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
                listOf(sigv4S3Express(codegenContext.runtimeConfig)),
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

    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> {
        // TODO(S3Express): Transfer a value of `s3_disable_express_session_auth` from `SdkConfig` and replace `None`
        //  with that value once aws-sdk-rust#1060 is implemented.
        return listOf(
            adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
                rustTemplate(
                    """${section.serviceConfigBuilder}.push_interceptor(#{DisableS3ExpressSessionAuthInterceptor}::new(#{None}).into_shared());""",
                    *preludeScope,
                    "DisableS3ExpressSessionAuthInterceptor" to s3ExpressModule(codegenContext.runtimeConfig).resolve("interceptors::DisableS3ExpressSessionAuthInterceptor"),
                )
            },
        )
    }
}

private class S3ExpressServiceRuntimePluginCustomization(codegenContext: ClientCodegenContext) :
    ServiceRuntimePluginCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope by lazy {
        arrayOf(
            *preludeScope,
            "DefaultS3ExpressIdentityProvider" to
                s3ExpressModule(runtimeConfig).resolve("identity_provider::DefaultS3ExpressIdentityProvider"),
            "DisableS3ExpressSessionAuthInterceptor" to s3ExpressModule(codegenContext.runtimeConfig).resolve("interceptors::DisableS3ExpressSessionAuthInterceptor"),
            "IdentityCacheLocation" to
                RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                    .resolve("client::identity::IdentityCacheLocation"),
            "S3ExpressAuthScheme" to
                s3ExpressModule(runtimeConfig).resolve("auth::S3ExpressAuthScheme"),
            "S3_EXPRESS_SCHEME_ID" to
                s3ExpressModule(runtimeConfig).resolve("auth::SCHEME_ID"),
            "SharedAuthScheme" to
                RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                    .resolve("client::auth::SharedAuthScheme"),
            "SharedCredentialsProvider" to
                configReexport(
                    AwsRuntimeType.awsCredentialTypes(runtimeConfig)
                        .resolve("provider::SharedCredentialsProvider"),
                ),
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
                            rustTemplate(
                                """
                                #{DefaultS3ExpressIdentityProvider}::builder()
                                    .time_source(${section.serviceConfigName}.time_source().unwrap_or_default())
                                    .build()
                                """,
                                *codegenScope,
                            )
                        },
                    )

                    // This interceptor may not be set if a user creates an S3 client without going through `SdkConfig`.
                    // If so, we set it here in a `ServiceRuntimePlugin`, and we deliberately specify `None` here because
                    // the interceptor created here does not go through `SdkConfig`, i.e. there is no
                    // `s3_disable_express_session_auth` value from a profile file.
                    // If the target interceptor has already been registered, we keep it (i.e. allow_duplicate = false).
                    section.registerInterceptor(this, allow_duplicate = false) {
                        rustTemplate("#{DisableS3ExpressSessionAuthInterceptor}::new(#{None})", *codegenScope)
                    }
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
                s3ExpressModule(runtimeConfig).resolve("auth::SCHEME_ID"),
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

private fun s3ExpressModule(runtimeConfig: RuntimeConfig) =
    RuntimeType.forInlineDependency(
        InlineAwsDependency.forRustFile(
            "s3_express",
            additionalDependency = s3ExpressDependencies(runtimeConfig),
        ),
    )

private fun s3ExpressDependencies(runtimeConfig: RuntimeConfig) =
    arrayOf(
        AwsCargoDependency.awsCredentialTypes(runtimeConfig),
        AwsCargoDependency.awsRuntime(runtimeConfig),
        AwsCargoDependency.awsSigv4(runtimeConfig),
        CargoDependency.Hex,
        CargoDependency.Hmac,
        CargoDependency.Lru,
        CargoDependency.Proptest,
        CargoDependency.Sha2,
        CargoDependency.smithyAsync(runtimeConfig),
        CargoDependency.smithyRuntimeApiClient(runtimeConfig),
        CargoDependency.smithyTypes(runtimeConfig),
    )
