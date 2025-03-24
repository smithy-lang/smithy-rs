/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.s3

import software.amazon.smithy.aws.traits.HttpChecksumTrait
import software.amazon.smithy.aws.traits.auth.SigV4Trait
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.configReexport
import software.amazon.smithy.rust.codegen.client.smithy.customize.AuthSchemeOption
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientSection
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
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rustsdk.AwsCargoDependency
import software.amazon.smithy.rustsdk.AwsRuntimeType
import software.amazon.smithy.rustsdk.InlineAwsDependency
import software.amazon.smithy.rustsdk.SdkConfigSection
import software.amazon.smithy.rustsdk.SigV4AuthDecorator

class S3ExpressDecorator : ClientCodegenDecorator {
    override val name: String = "S3ExpressDecorator"

    // This decorator must decorate after SigV4AuthDecorator so that sigv4 appears before sigv4-s3express within auth_scheme_options
    override val order: Byte = (SigV4AuthDecorator.ORDER - 1).toByte()

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

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> =
        baseCustomizations +
            S3ExpressRequestChecksumCustomization(
                codegenContext, operation,
            )

    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> {
        return listOf(
            adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
                rust(
                    """
                    ${section.serviceConfigBuilder}.set_disable_s3_express_session_auth(
                        ${section.sdkConfig}
                            .service_config()
                            .and_then(|conf| {
                                let str_config = conf.load_config(service_config_key("AWS_S3_DISABLE_EXPRESS_SESSION_AUTH", "s3_disable_express_session_auth"));
                                str_config.and_then(|it| it.parse::<bool>().ok())
                            }),
                    );
                    """,
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
            "DefaultS3ExpressIdentityProvider" to
                s3ExpressModule(runtimeConfig).resolve("identity_provider::DefaultS3ExpressIdentityProvider"),
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
                }

                else -> {}
            }
        }
}

private class S3ExpressIdentityProviderConfig(codegenContext: ClientCodegenContext) : ConfigCustomization() {
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

class S3ExpressFluentClientCustomization(
    codegenContext: ClientCodegenContext,
) : FluentClientCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "S3ExpressRuntimePlugin" to s3ExpressModule(runtimeConfig).resolve("runtime_plugin::S3ExpressRuntimePlugin"),
        )

    override fun section(section: FluentClientSection): Writable =
        writable {
            when (section) {
                is FluentClientSection.AdditionalBaseClientPlugins -> {
                    rustTemplate(
                        """
                        ${section.plugins} = ${section.plugins}.with_client_plugin(
                            #{S3ExpressRuntimePlugin}::new(${section.config}.clone())
                        );
                        """,
                        *codegenScope,
                    )
                }

                else -> emptySection
            }
        }
}

class S3ExpressRequestChecksumCustomization(
    codegenContext: ClientCodegenContext,
    private val operationShape: OperationShape,
) : OperationCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig

    private val codegenScope =
        arrayOf(
            *preludeScope,
            "ChecksumAlgorithm" to RuntimeType.smithyChecksums(runtimeConfig).resolve("ChecksumAlgorithm"),
            "ConfigBag" to RuntimeType.configBag(runtimeConfig),
            "Document" to RuntimeType.smithyTypes(runtimeConfig).resolve("Document"),
            "for_s3_express" to s3ExpressModule(runtimeConfig).resolve("utils::for_s3_express"),
            "provide_default_checksum_algorithm" to s3ExpressModule(runtimeConfig).resolve("checksum::provide_default_checksum_algorithm"),
        )

    override fun section(section: OperationSection): Writable =
        writable {
            // Get the `HttpChecksumTrait`, returning early if this `OperationShape` doesn't have one
            val checksumTrait = operationShape.getTrait<HttpChecksumTrait>() ?: return@writable
            when (section) {
                is OperationSection.AdditionalRuntimePluginConfig -> {
                    if (checksumTrait.isRequestChecksumRequired) {
                        rustTemplate(
                            """
                            ${section.newLayerName}.store_put(#{provide_default_checksum_algorithm}());
                            """,
                            *codegenScope,
                            "customDefault" to
                                writable {
                                    rustTemplate("#{Some}(#{ChecksumAlgorithm}::Crc32)", *codegenScope)
                                },
                        )
                    }
                }

                else -> { }
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
        CargoDependency.FastRand,
        CargoDependency.Hex,
        CargoDependency.Hmac,
        CargoDependency.Lru,
        CargoDependency.Sha2,
        CargoDependency.smithyAsync(runtimeConfig),
        CargoDependency.smithyChecksums(runtimeConfig),
        CargoDependency.smithyRuntimeApiClient(runtimeConfig),
        CargoDependency.smithyTypes(runtimeConfig),
    )
