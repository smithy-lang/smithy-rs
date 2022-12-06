/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

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
        val codegenScope = arrayOf(
            "SdkConfig" to AwsRuntimeType.awsTypes(codegenContext.runtimeConfig).resolve("sdk_config::SdkConfig"),
        )
        rustCrate.withModule(RustModule.Config) {
            // !!NOTE!! As more items are added to aws_types::SdkConfig, use them here to configure the config builder
            rustTemplate(
                """
                impl From<&#{SdkConfig}> for Builder {
                    fn from(input: &#{SdkConfig}) -> Self {
                        let mut builder = Builder::default();
                        builder = builder.region(input.region().cloned());
                        builder.set_endpoint_resolver(input.endpoint_resolver().clone());
                        builder.set_retry_config(input.retry_config().cloned());
                        builder.set_timeout_config(input.timeout_config().cloned());
                        builder.set_sleep_impl(input.sleep_impl());
                        builder.set_credentials_provider(input.credentials_provider().cloned());
                        builder.set_app_name(input.app_name().cloned());
                        builder.set_http_connector(input.http_connector().cloned());
                        builder
                    }
                }

                impl From<&#{SdkConfig}> for Config {
                    fn from(sdk_config: &#{SdkConfig}) -> Self {
                        Builder::from(sdk_config).build()
                    }
                }
                """,
                *codegenScope,
            )
        }
    }

    override fun supportsCodegenContext(clazz: Class<out CodegenContext>): Boolean =
        clazz.isAssignableFrom(ClientCodegenContext::class.java)
}

class NewFromShared(runtimeConfig: RuntimeConfig) : ConfigCustomization() {
    private val codegenScope = arrayOf(
        "SdkConfig" to AwsRuntimeType.awsTypes(runtimeConfig).resolve("sdk_config::SdkConfig"),
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
                    *codegenScope,
                )
            }
            else -> emptySection
        }
    }
}
