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
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.smithy.letIf
import software.amazon.smithy.rust.codegen.util.dq

/**
 * The SigV4SigningDecorator:
 * - adds a `signing_service()` method to `config` to return the default signing service
 * - sets the `SigningService` during operation construction
 * - sets a default `OperationSigningConfig` A future enhancement will customize this for specific services that need
 *   different behavior.
 */
class SigV4SigningDecorator : RustCodegenDecorator {
    override val name: String = "SigV4Signing"
    override val order: Byte = 0

    private fun applies(protocolConfig: ProtocolConfig): Boolean = protocolConfig.serviceShape.hasTrait(SigV4Trait::class.java)

    override fun configCustomizations(
        protocolConfig: ProtocolConfig,
        baseCustomizations: List<ConfigCustomization>
    ): List<ConfigCustomization> {
        return baseCustomizations.letIf(applies(protocolConfig)) {
            it + SigV4SigningConfig(protocolConfig.serviceShape.expectTrait(SigV4Trait::class.java))
        }
    }

    override fun operationCustomizations(
        protocolConfig: ProtocolConfig,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>
    ): List<OperationCustomization> {
        return baseCustomizations.letIf(applies(protocolConfig)) {
            it + SigV4SigningFeature(protocolConfig.runtimeConfig)
        }
    }
}

class SigV4SigningConfig(private val sigV4Trait: SigV4Trait) : ConfigCustomization() {
    override fun section(section: ServiceConfig): Writable {
        return when (section) {
            is ServiceConfig.ConfigImpl -> writable {
                rust(
                    """
                    /// The signature version 4 service signing name to use in the credential scope when signing requests.
                    ///
                    /// The signing service may be overidden by the `Endpoint`, or by specifying a custom [`SigningService`](aws_types::SigningService) during
                    /// operation construction
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

class SigV4SigningFeature(private val runtimeConfig: RuntimeConfig) :
    OperationCustomization() {
    override fun section(section: OperationSection): Writable {
        return when (section) {
            is OperationSection.Feature -> writable {
                // TODO: this needs to be customized for individual operations, not just `default_config()`
                rustTemplate(
                    """
                ${section.request}.config_mut().insert(
                    #{sig_auth}::signer::OperationSigningConfig::default_config()
                );
                ${section.request}.config_mut().insert(#{aws_types}::SigningService::from_static(${section.config}.signing_service()));
                """,
                    "sig_auth" to runtimeConfig.sigAuth().asType(),
                    "aws_types" to awsTypes(runtimeConfig).asType()
                )
            }
            else -> emptySection
        }
    }
}

fun RuntimeConfig.sigAuth() = CargoDependency("aws-sig-auth", Local(this.relativePath))
