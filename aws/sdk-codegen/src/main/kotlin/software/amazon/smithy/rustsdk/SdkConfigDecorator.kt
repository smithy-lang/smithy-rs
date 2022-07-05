/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfig

/**
 * Adds functionality for constructing `<service>::Config` objects from `aws_types::SdkConfig`s
 *
 * - `From<&aws_types::SdkConfig> for <service>::config::Builder`: Enabling customization
 * - `pub fn new(&aws_types::SdkConfig) -> <service>::Config`: Direct construction without customization
 */
class SdkConfigDecorator : RustCodegenDecorator<ClientCodegenContext> {
    override val name: String = "SdkConfig"
    override val order: Byte = 0

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>
    ): List<ConfigCustomization> {
        return baseCustomizations + NewFromShared(codegenContext.runtimeConfig)
    }

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        val codegenScope = arrayOf(
            "SdkConfig" to awsTypes(runtimeConfig = codegenContext.runtimeConfig).asType().member("sdk_config::SdkConfig")
        )
        rustCrate.withModule(RustModule.Config) {
            // !!NOTE!! As more items are added to aws_types::SdkConfig, use them here to configure the config builder
            it.rustTemplate(
                """
                impl From<&#{SdkConfig}> for Builder {
                    fn from(input: &#{SdkConfig}) -> Self {
                        let mut builder = Builder::default();
                        builder = builder.region(input.region().cloned());
                        builder.set_endpoint_resolver(input.endpoint_resolver().clone());
                        builder.set_retry_config(input.retry_config().cloned());
                        builder.set_timeout_config(input.timeout_config().cloned());
                        builder.set_sleep_impl(input.sleep_impl().clone());
                        builder.set_credentials_provider(input.credentials_provider().cloned());
                        builder.set_app_name(input.app_name().cloned());
                        builder
                    }
                }

                impl From<&#{SdkConfig}> for Config {
                    fn from(sdk_config: &#{SdkConfig}) -> Self {
                        Builder::from(sdk_config).build()
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
        "SdkConfig" to awsTypes(runtimeConfig = runtimeConfig).asType().member("sdk_config::SdkConfig")
    )
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
                    *codegenScope
                )
            }
            else -> emptySection
        }
    }
}
