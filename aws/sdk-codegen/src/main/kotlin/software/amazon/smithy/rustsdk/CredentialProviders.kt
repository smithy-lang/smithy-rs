/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsSection

class CredentialsProviderDecorator : ClientCodegenDecorator {
    override val name: String = "CredentialsProvider"
    override val order: Byte = 0

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations + CredentialProviderConfig(codegenContext.runtimeConfig)
    }

    override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> {
        return baseCustomizations + PubUseCredentials(codegenContext.runtimeConfig)
    }
}

/**
 * Add a `.credentials_provider` field and builder to the `Config` for a given service
 */
class CredentialProviderConfig(runtimeConfig: RuntimeConfig) : ConfigCustomization() {
    private val codegenScope = arrayOf(
        "provider" to AwsRuntimeType.awsCredentialTypes(runtimeConfig).resolve("provider"),
        "DefaultProvider" to defaultProvider(),
    )

    override fun section(section: ServiceConfig) = writable {
        when (section) {
            ServiceConfig.BuilderStruct ->
                rustTemplate("credentials_provider: Option<std::sync::Arc<dyn #{provider}::ProvideCredentials>>,", *codegenScope)
            ServiceConfig.BuilderImpl -> {
                rustTemplate(
                    """
                    /// Sets the credentials provider for this service
                    pub fn credentials_provider(mut self, credentials_provider: impl #{provider}::ProvideCredentials + 'static) -> Self {
                        self.set_credentials_provider(Some(std::sync::Arc::new(credentials_provider)));
                        self
                    }

                    /// Sets the credentials provider for this service
                    pub fn set_credentials_provider(&mut self, credentials_provider: Option<std::sync::Arc<dyn #{provider}::ProvideCredentials>>) -> &mut Self {
                        self.credentials_provider = credentials_provider;
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

class PubUseCredentials(private val runtimeConfig: RuntimeConfig) : LibRsCustomization() {
    override fun section(section: LibRsSection): Writable {
        return when (section) {
            is LibRsSection.Body -> writable {
                rust(
                    "pub use #T::Credentials;",
                    AwsRuntimeType.awsCredentialTypes(runtimeConfig),
                )
            }

            else -> emptySection
        }
    }
}

fun defaultProvider() =
    RuntimeType.forInlineDependency(InlineAwsDependency.forRustFile("no_credentials")).resolve("NoCredentials")
