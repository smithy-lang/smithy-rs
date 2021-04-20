/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.docs
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.OperationSection
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
    private val credentialsProvider = credentialsProvider(runtimeConfig)
    private val defaultProvider = defaultProvider(runtimeConfig)
    override fun section(section: ServiceConfig) = writable {
        when (section) {
            is ServiceConfig.ConfigStruct -> rust(
                """pub(crate) credentials_provider: std::sync::Arc<dyn #T>,""",
                credentialsProvider
            )
            is ServiceConfig.ConfigImpl -> emptySection
            is ServiceConfig.BuilderStruct ->
                rust("credentials_provider: Option<std::sync::Arc<dyn #T>>,", credentialsProvider)
            ServiceConfig.BuilderImpl -> {
                docs("""Set the credentials provider for this service""")
                rust(
                    """
            pub fn credentials_provider(mut self, credentials_provider: impl #T + 'static) -> Self {
                self.credentials_provider = Some(std::sync::Arc::new(credentials_provider));
                self
            }
            """,
                    credentialsProvider,
                )
            }
            ServiceConfig.BuilderBuild -> rust(
                "credentials_provider: self.credentials_provider.unwrap_or_else(|| std::sync::Arc::new(#T())),",
                defaultProvider
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
                #T(&mut ${section.request}.config_mut(), ${section.config}.credentials_provider.clone());
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
            is LibRsSection.Body -> writable { rust("pub use #T::Credentials;", awsAuth(runtimeConfig).asType()) }
            else -> emptySection
        }
    }
}

fun awsAuth(runtimeConfig: RuntimeConfig) = runtimeConfig.awsRuntimeDependency("aws-auth")
fun credentialsProvider(runtimeConfig: RuntimeConfig) =
    RuntimeType("ProvideCredentials", awsAuth(runtimeConfig), "aws_auth")

fun defaultProvider(runtimeConfig: RuntimeConfig) = RuntimeType("default_provider", awsAuth(runtimeConfig), "aws_auth")
fun setProvider(runtimeConfig: RuntimeConfig) = RuntimeType("set_provider", awsAuth(runtimeConfig), "aws_auth")
