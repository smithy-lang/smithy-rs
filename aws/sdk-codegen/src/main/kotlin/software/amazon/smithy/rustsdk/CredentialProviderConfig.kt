/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.Local
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfig

/**
 * Just a Stub
 *
 * Augment the config object with the AWS-specific fields like service and region
 */
class CredentialProviderConfig : ConfigCustomization() {
    override fun section(section: ServiceConfig) = writable {
        when (section) {
            is ServiceConfig.ConfigStruct -> rust("pub credentials_provider: ::std::sync::Arc<dyn #T>,", CredentialsProvider)
            is ServiceConfig.ConfigImpl -> emptySection
            is ServiceConfig.BuilderStruct ->
                rust("credentials_provider: Option<::std::sync::Arc<dyn #T>>,", CredentialsProvider)
            ServiceConfig.BuilderImpl ->
                rust(
                    """
            pub fn credentials_provider(mut self, credentials_provider: impl #T + 'static) -> Self {
                self.credentials_provider = Some(::std::sync::Arc::new(credentials_provider));
                self
            }
            """,
                    CredentialsProvider
                )
            ServiceConfig.BuilderBuild -> rust(
                "credentials_provider: self.credentials_provider.unwrap_or_else(|| ::std::sync::Arc::new(#T())),",
                DefaultProvider
            )
        }
    }
}

val Auth = CargoDependency("aws-auth", Local("../"))
val CredentialsProvider = RuntimeType("ProvideCredentials", Auth, "aws_auth")
val DefaultProvider = RuntimeType("default_provider", Auth, "aws_auth")
