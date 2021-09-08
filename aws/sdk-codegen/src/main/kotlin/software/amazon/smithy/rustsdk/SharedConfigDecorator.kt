/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfig

/**
 * Adds functionality for constructing <service>::Config objects from aws_types::Config (SharedConfig)
 *
 * - `From<&aws_types::Config> for <service>::config::Builder`: Enabling customization
 * - `pub fn new(&aws_types::Config) -> <service>::Config`: Direct construction without customization
 */
class SharedConfigDecorator : RustCodegenDecorator {
    override val name: String = "SharedConfig"
    override val order: Byte = 0

    override fun configCustomizations(
        protocolConfig: ProtocolConfig,
        baseCustomizations: List<ConfigCustomization>
    ): List<ConfigCustomization> {
        return baseCustomizations + NewFromShared(protocolConfig.runtimeConfig)
    }

    override fun extras(protocolConfig: ProtocolConfig, rustCrate: RustCrate) {
        val codegenScope = arrayOf(
            "Config" to awsTypes(runtimeConfig = protocolConfig.runtimeConfig).asType().member("config::Config")
        )
        rustCrate.withModule(RustModule.Config) {
            // TODO(sharedconfig): As more items are added to aws_types::Config, use them here to configure the config builder
            it.rustTemplate(
                """
                impl From<&#{Config}> for Builder {
                    fn from(input: &#{Config}) -> Self {
                        let mut builder = Builder::default();
                        builder = builder.region(input.region().cloned());
                        builder.set_credentials_provider(input.credentials_provider().cloned());
                        builder
                    }
                }

                impl From<&#{Config}> for Config {
                    fn from(config: &#{Config}) -> Self {
                        Builder::from(config).build()
                    }
                }
            """,
                *codegenScope
            )
        }
    }
}

class NewFromShared(runtimeConfig: RuntimeConfig) : ConfigCustomization() {
    private val codegenScope = arrayOf(
        "Config" to awsTypes(runtimeConfig = runtimeConfig).asType().member("config::Config")
    )
    override fun section(section: ServiceConfig): Writable {
        return when (section) {
            ServiceConfig.ConfigImpl -> writable {
                rustTemplate(
                    """
                    pub fn new(config: &#{Config}) -> Self {
                        Builder::from(config).build()
                    }
                """,
                    *codegenScope
                )
            }
            else -> emptySection
        }
    }
}
