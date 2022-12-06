/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolGenerator
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

class CredentialsProviderDecorator : RustCodegenDecorator<ClientProtocolGenerator, ClientCodegenContext> {
    override val name: String = "CredentialsProvider"
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

    override fun supportsCodegenContext(clazz: Class<out CodegenContext>): Boolean =
        clazz.isAssignableFrom(ClientCodegenContext::class.java)
}

/**
 * Add a `.credentials_provider` field and builder to the `Config` for a given service
 */
class CredentialProviderConfig(runtimeConfig: RuntimeConfig) : ConfigCustomization() {
    private val defaultProvider = defaultProvider()
    private val codegenScope = arrayOf(
        "credentials" to AwsRuntimeType.awsTypes(runtimeConfig).resolve("credentials"),
        "DefaultProvider" to defaultProvider,
    )

    override fun section(section: ServiceConfig) = writable {
        when (section) {
            ServiceConfig.ConfigStruct -> rustTemplate(
                """pub(crate) credentials_provider: #{credentials}::SharedCredentialsProvider,""",
                *codegenScope,
            )
            ServiceConfig.ConfigImpl -> rustTemplate(
                """
                /// Returns the credentials provider.
                pub fn credentials_provider(&self) -> #{credentials}::SharedCredentialsProvider {
                    self.credentials_provider.clone()
                }
                """,
                *codegenScope,
            )
            ServiceConfig.BuilderStruct ->
                rustTemplate("credentials_provider: Option<#{credentials}::SharedCredentialsProvider>,", *codegenScope)
            ServiceConfig.BuilderImpl -> {
                rustTemplate(
                    """
                    /// Sets the credentials provider for this service
                    pub fn credentials_provider(mut self, credentials_provider: impl #{credentials}::ProvideCredentials + 'static) -> Self {
                        self.credentials_provider = Some(#{credentials}::SharedCredentialsProvider::new(credentials_provider));
                        self
                    }

                    /// Sets the credentials provider for this service
                    pub fn set_credentials_provider(&mut self, credentials_provider: Option<#{credentials}::SharedCredentialsProvider>) -> &mut Self {
                        self.credentials_provider = credentials_provider;
                        self
                    }
                    """,
                    *codegenScope,
                )
            }

            ServiceConfig.BuilderBuild -> rustTemplate(
                "credentials_provider: self.credentials_provider.unwrap_or_else(|| #{credentials}::SharedCredentialsProvider::new(#{DefaultProvider})),",
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
                    setProvider(runtimeConfig),
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
                    "pub use #T::Credentials;",
                    AwsRuntimeType.awsTypes(runtimeConfig),
                )
            }

            else -> emptySection
        }
    }
}

fun defaultProvider() =
    RuntimeType.forInlineDependency(InlineAwsDependency.forRustFile("no_credentials")).resolve("NoCredentials")

fun setProvider(runtimeConfig: RuntimeConfig) = AwsRuntimeType.awsHttp(runtimeConfig).resolve("auth::set_provider")
