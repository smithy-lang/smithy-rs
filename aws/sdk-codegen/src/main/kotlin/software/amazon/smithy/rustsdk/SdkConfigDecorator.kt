/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocSection
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.util.dq

sealed class SdkConfigSection(name: String) : AdHocSection(name) {
    /**
     * [sdkConfig]: A reference to the SDK config struct
     * [serviceConfigBuilder]: A reference (owned) to the `<service>::config::Builder` struct.
     *
     * Each invocation of this section MUST be a complete statement (ending with a semicolon), e.g:
     * ```
     * rust("${section.serviceConfigBuilder}.set_foo(${section.sdkConfig}.foo());")
     * ```
     */
    data class CopySdkConfigToClientConfig(val sdkConfig: String, val serviceConfigBuilder: String) :
        SdkConfigSection("CopySdkConfigToClientConfig") {
        fun inheritFieldOriginFromSdkConfig(
            writer: RustWriter,
            fieldName: String,
        ) {
            writer.rust(
                """
                $serviceConfigBuilder
                    .config_origins
                    .insert(${fieldName.dq()}, $sdkConfig.get_origin(${fieldName.dq()}));
                """,
            )
        }
    }
}

/**
 * Section enabling linkage between `SdkConfig` and <service>::Config
 */
object SdkConfigCustomization {
    /**
     * Copy a field from SDK config to service config with an optional map block.
     *
     * This handles the common case where the field name is identical in both cases and an accessor is used.
     *
     * # Examples
     * ```kotlin
     * SdkConfigCustomization.copyField("some_string_field") { rust("|s|s.to_to_string()") }
     * ```
     */
    fun copyField(
        fieldName: String,
        map: Writable?,
        trackOrigin: (section: SdkConfigSection.CopySdkConfigToClientConfig, w: RustWriter) -> Unit,
    ) = adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
        val mapBlock = map?.let { writable { rust(".map(#W)", it) } } ?: writable { }
        rustTemplate(
            "${section.serviceConfigBuilder}.set_$fieldName(${section.sdkConfig}.$fieldName()#{map});",
            "map" to mapBlock,
        )
        trackOrigin(section, this)
    }
}

/**
 * SdkConfig -> <service>::Config for settings that come from generic smithy
 */
class GenericSmithySdkConfigSettings : ClientCodegenDecorator {
    override val name: String = "GenericSmithySdkConfigSettings"
    override val order: Byte = 0

    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> =
        listOf(
            adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
                rust(
                    """
                    // resiliency
                    ${section.serviceConfigBuilder}.set_retry_config(${section.sdkConfig}.retry_config().cloned());
                    ${section.serviceConfigBuilder}.set_timeout_config(${section.sdkConfig}.timeout_config().cloned());
                    ${section.serviceConfigBuilder}.set_sleep_impl(${section.sdkConfig}.sleep_impl());

                    ${section.serviceConfigBuilder}.set_http_client(${section.sdkConfig}.http_client());
                    ${section.serviceConfigBuilder}.set_time_source(${section.sdkConfig}.time_source());
                    ${section.serviceConfigBuilder}.set_behavior_version(${section.sdkConfig}.behavior_version());
                    ${section.serviceConfigBuilder}.set_auth_scheme_preference(${section.sdkConfig}.auth_scheme_preference().cloned());
                    // setting `None` here removes the default
                    if let Some(config) = ${section.sdkConfig}.stalled_stream_protection() {
                        ${section.serviceConfigBuilder}.set_stalled_stream_protection(Some(config));
                    }

                    if let Some(cache) = ${section.sdkConfig}.identity_cache() {
                        ${section.serviceConfigBuilder}.set_identity_cache(cache);
                    }
                    """,
                )
            },
        )
}

/**
 * Adds functionality for constructing `<service>::Config` objects from `aws_types::SdkConfig`s
 *
 * - `From<&aws_types::SdkConfig> for <service>::config::Builder`: Enabling customization
 * - `pub fn new(&aws_types::SdkConfig) -> <service>::Config`: Direct construction without customization
 */
class SdkConfigDecorator : ClientCodegenDecorator {
    override val name: String = "SdkConfig"
    override val order: Byte = 0

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations + NewFromShared(codegenContext.runtimeConfig)
    }

    override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        val codegenScope =
            arrayOf(
                "SdkConfig" to AwsRuntimeType.awsTypes(codegenContext.runtimeConfig).resolve("sdk_config::SdkConfig"),
            )

        rustCrate.withModule(ClientRustModule.config) {
            rustTemplate(
                """
                impl From<&#{SdkConfig}> for Builder {
                    fn from(input: &#{SdkConfig}) -> Self {
                        let mut builder = Builder::default();
                        #{augmentBuilder:W}

                        builder
                    }
                }

                impl From<&#{SdkConfig}> for Config {
                    fn from(sdk_config: &#{SdkConfig}) -> Self {
                        Builder::from(sdk_config).build()
                    }
                }
                """,
                "augmentBuilder" to
                    writable {
                        writeCustomizations(
                            codegenContext.rootDecorator.extraSections(codegenContext),
                            SdkConfigSection.CopySdkConfigToClientConfig(
                                sdkConfig = "input",
                                serviceConfigBuilder = "builder",
                            ),
                        )
                    },
                *codegenScope,
            )
        }
    }
}

class NewFromShared(runtimeConfig: RuntimeConfig) : ConfigCustomization() {
    private val codegenScope =
        arrayOf(
            "SdkConfig" to AwsRuntimeType.awsTypes(runtimeConfig).resolve("sdk_config::SdkConfig"),
        )

    override fun section(section: ServiceConfig): Writable {
        return when (section) {
            ServiceConfig.ConfigImpl ->
                writable {
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
