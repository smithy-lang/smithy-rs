/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.aws.traits.auth.SigV4Trait
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.Local
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.util.dq

class SigV4SigningConfig(private val sigV4Trait: SigV4Trait) : ConfigCustomization() {
    override fun section(section: ServiceConfig): Writable {
        return when (section) {
            is ServiceConfig.ConfigImpl -> writable {
                rust(
                    """
                    /// The signature version 4 service signing name to use in the credential scope when signing requests.
                    pub fn signing_service(&self) -> &'static str {
                        ${sigV4Trait.name.dq()}
                    }
                    """
                )
            }
            else -> emptySection
        }
    }
}

class SigV4SigningPlugin(operationShape: OperationShape) : OperationCustomization() {
    override fun section(section: OperationSection): Writable {
        return when (section) {
            is OperationSection.Plugin -> writable {
                rust(
                    """
                use operationwip::signing_middleware::SigningConfigExt;
                request.config_mut().insert_signing_config(
                    #T::OperationSigningConfig::default_config(_config.signing_service())
                );
                use operationwip::signing_middleware::CredentialProviderExt;
                request.config_mut().insert_credentials_provider(_config.credentials_provider.clone());
                """,
                    awsSigAuth
                )
            }
            else -> emptySection
        }
    }
}

val SigAuth = CargoDependency("aws-sig-auth", Local("../"))
val awsSigAuth = RuntimeType(null, SigAuth, "aws_sig_auth")
