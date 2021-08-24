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
            it.rustTemplate(
                """
                impl From<&#{Config}> for Builder {
                    fn from(input: &#{Config}) -> Self {
                        let mut builder = Builder::default();
                        builder = builder.region(input.region().cloned());
                        if let Some(provider) = input.credentials_provider() {
                            builder = builder.credentials_provider(provider);
                        }
                        builder
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
