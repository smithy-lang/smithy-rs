/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.decorators

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rustsdk.awsTypes

/**
 * Adds functionality for constructing `<service>::Config` objects from `aws_types::SdkConfig`s
 *
 * - `From<&aws_types::SdkConfig> for <service>::config::Builder`: Enabling customization
 * - `pub fn new(&aws_types::SdkConfig) -> <service>::Config`: Direct construction without customization
 */
class SdkConfigDecorator : RustCodegenDecorator<ClientProtocolGenerator, ClientCodegenContext> {
    override val name: String = "SdkConfig"
    override val order: Byte = 0

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations + NewFromShared(codegenContext.runtimeConfig)
    }

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        rustCrate.withModule(RustModule.Config) {
            rustTemplate(
                """
                impl From<&#{SdkConfig}> for Config {
                    fn from(sdk_config: &#{SdkConfig}) -> Self {
                        Builder::from(sdk_config).build()
                    }
                }
                """,
                "SdkConfig" to codegenContext.runtimeConfig.awsTypes().member("sdk_config::SdkConfig"),
            )
        }
    }

    override fun supportsCodegenContext(clazz: Class<out CodegenContext>): Boolean =
        clazz.isAssignableFrom(ClientCodegenContext::class.java)
}

class NewFromShared(private val runtimeConfig: RuntimeConfig) : ConfigCustomization() {
    override fun section(section: ServiceConfig): Writable {
        return when (section) {
            ServiceConfig.ConfigImpl -> writable {
                rustTemplate(
                    """
                    /// Creates a new [service config](crate::Config) from a [shared `config`](#{SdkConfig}).
                    pub fn new(config: &#{SdkConfig}) -> Self {
                        Builder::from(config).build()
                    }
                    """,
                    "SdkConfig" to runtimeConfig.awsTypes().member("sdk_config::SdkConfig"),
                )
            }
            else -> emptySection
        }
    }
}
