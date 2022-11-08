/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.decorators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsSection
import software.amazon.smithy.rustsdk.AwsCustomization
import software.amazon.smithy.rustsdk.AwsSection
import software.amazon.smithy.rustsdk.InlineAwsDependency
import software.amazon.smithy.rustsdk.awsHttp
import software.amazon.smithy.rustsdk.awsTypes

class CredentialsProviderDecorator : AwsCodegenDecorator {
    override val order: Byte = 0

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations + CredentialProviderConfig(codegenContext.runtimeConfig)
    }

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        return baseCustomizations + CredentialsProviderFeature(codegenContext.runtimeConfig)
    }

    override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> {
        return baseCustomizations + PubUseCredentials(codegenContext.runtimeConfig)
    }

    override fun awsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<AwsCustomization>,
    ): List<AwsCustomization> {
        return baseCustomizations + CredentialsFromSdkConfig()
    }

    override fun supportsCodegenContext(clazz: Class<out CodegenContext>): Boolean =
        clazz.isAssignableFrom(ClientCodegenContext::class.java)
}

/**
 * Add a `.credentials_provider` field and builder to the `Config` for a given service
 */
class CredentialProviderConfig(runtimeConfig: RuntimeConfig) : ConfigCustomization() {
    private val defaultProvider =
        RuntimeType.forInlineDependency(InlineAwsDependency.forRustFile("no_credentials")).member("NoCredentials")
    private val codegenScope = arrayOf(
        "ProvideCredentials" to awsTypes(runtimeConfig).member("credentials::ProvideCredentials"),
        "SharedCredentialsProvider" to awsTypes(runtimeConfig).member("credentials::SharedCredentialsProvider"),
        "DefaultProvider" to defaultProvider,
    )

    override fun section(section: ServiceConfig) = writable {
        when (section) {
            is ServiceConfig.ConfigStruct -> rustTemplate(
                """pub(crate) credentials_provider: #{SharedCredentialsProvider},""",
                *codegenScope,
            )

            is ServiceConfig.ConfigImpl -> rustTemplate(
                """
                /// Returns the credentials provider.
                pub fn credentials_provider(&self) -> #{SharedCredentialsProvider} {
                    self.credentials_provider.clone()
                }
                """,
                *codegenScope,
            )

            is ServiceConfig.BuilderStruct ->
                rustTemplate("credentials_provider: Option<#{SharedCredentialsProvider}>,", *codegenScope)

            ServiceConfig.BuilderImpl -> {
                rustTemplate(
                    """
                    /// Sets the credentials provider for this service
                    pub fn credentials_provider(mut self, credentials_provider: impl #{ProvideCredentials} + 'static) -> Self {
                        self.credentials_provider = Some(#{SharedCredentialsProvider}::new(credentials_provider));
                        self
                    }

                    /// Sets the credentials provider for this service
                    pub fn set_credentials_provider(&mut self, credentials_provider: Option<#{SharedCredentialsProvider}>) -> &mut Self {
                        self.credentials_provider = credentials_provider;
                        self
                    }
                    """,
                    *codegenScope,
                )
            }

            ServiceConfig.BuilderBuild -> rustTemplate(
                "credentials_provider: self.credentials_provider.unwrap_or_else(|| #{SharedCredentialsProvider}::new(#{DefaultProvider})),",
                *codegenScope,
            )

            else -> emptySection
        }
    }
}

class CredentialsProviderFeature(private val runtimeConfig: RuntimeConfig) : OperationCustomization() {
    override fun section(section: OperationSection): Writable {
        return when (section) {
            is OperationSection.MutateRequest -> writable {
                rust(
                    """
                    #T(&mut ${section.request}.properties_mut(), ${section.config}.credentials_provider.clone());
                    """,
                    awsHttp(runtimeConfig).member("auth::set_provider"),
                )
            }

            else -> emptySection
        }
    }
}

class PubUseCredentials(private val runtimeConfig: RuntimeConfig) : LibRsCustomization() {
    override fun section(section: LibRsSection): Writable {
        return when (section) {
            is LibRsSection.Body -> writable {
                rust(
                    "pub use #T;",
                    awsTypes(runtimeConfig).member("Credentials"),
                )
            }

            else -> emptySection
        }
    }
}

class CredentialsFromSdkConfig : AwsCustomization() {
    override fun section(section: AwsSection): Writable = writable {
        when (section) {
            is AwsSection.FromSdkConfigForBuilder -> rust("builder.set_credentials_provider(input.credentials_provider().cloned());")
        }
    }
}
