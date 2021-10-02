/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfig

class CredentialsProviderDecorator : RustCodegenDecorator {
    override val name: String = "CredentialsProvider"
    override val order: Byte = 0

    override fun configCustomizations(
        protocolConfig: ProtocolConfig,
        baseCustomizations: List<ConfigCustomization>
    ): List<ConfigCustomization> {
        return baseCustomizations + CredentialProviderConfig(protocolConfig.runtimeConfig)
    }

    override fun operationCustomizations(
        protocolConfig: ProtocolConfig,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>
    ): List<OperationCustomization> {
        return baseCustomizations + CredentialsProviderFeature(protocolConfig.runtimeConfig)
    }

    override fun libRsCustomizations(
        protocolConfig: ProtocolConfig,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> {
        return baseCustomizations + PubUseCredentials(protocolConfig.runtimeConfig)
    }
}

/**
 * Add a `.credentials_provider` field and builder to the `Config` for a given service
 */
class CredentialProviderConfig(runtimeConfig: RuntimeConfig) : ConfigCustomization() {
    private val defaultProvider = defaultProvider()
    private val codegenScope = arrayOf(
        "credentials" to awsTypes(runtimeConfig).asType().member("credentials"),
        "DefaultProvider" to defaultProvider
    )

    override fun section(section: ServiceConfig) = writable {
        when (section) {
            is ServiceConfig.ConfigStruct -> rustTemplate(
                """pub(crate) credentials_provider: #{credentials}::SharedCredentialsProvider,""",
                *codegenScope
            )
            is ServiceConfig.ConfigImpl -> emptySection
            is ServiceConfig.BuilderStruct ->
                rustTemplate("credentials_provider: Option<#{credentials}::SharedCredentialsProvider>,", *codegenScope)
            ServiceConfig.BuilderImpl -> {
                rustTemplate(
                    """
                    /// Set the credentials provider for this service
                    pub fn credentials_provider(mut self, credentials_provider: impl #{credentials}::ProvideCredentials + 'static) -> Self {
                        self.credentials_provider = Some(#{credentials}::SharedCredentialsProvider::new(credentials_provider));
                        self
                    }

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
                *codegenScope
            )
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
                    setProvider(runtimeConfig)
                )
            }
            else -> emptySection
        }
    }
}

class PubUseCredentials(private val runtimeConfig: RuntimeConfig) : LibRsCustomization() {
    override fun section(section: LibRsSection): Writable {
        return when (section) {
            is LibRsSection.Body -> writable { rust("pub use #T::Credentials;", awsTypes(runtimeConfig).asType()) }
            else -> emptySection
        }
    }
}

fun awsAuth(runtimeConfig: RuntimeConfig) = runtimeConfig.awsRuntimeDependency("aws-auth")

fun defaultProvider() =
    RuntimeType.forInlineDependency(InlineAwsDependency.forRustFile("no_credentials")).member("NoCredentials")

fun setProvider(runtimeConfig: RuntimeConfig) =
    RuntimeType("set_provider", awsAuth(runtimeConfig), "aws_auth")
