/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.configReexport
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocSection
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations

sealed class SdkConfigSection(name: String) : AdHocSection(name) {
    /**
     * [sdkConfig]: A reference to the SDK config struct
     * [serviceConfigBuilder]: A reference (owned) to the `<service>::config::Builder` struct.
     *
     * This section is used within `From<&SdkConfig> for config::Builder` to copy
     * the provided `SdkConfig` into a field of the Builder, e.g:
     * ```
     * builder.sdk_config = Some(input.clone());
     * ```
     */
    data class CopySdkConfigToClientConfig(val sdkConfig: String, val serviceConfigBuilder: String) :
        SdkConfigSection("CopySdkConfigToClientConfig")
}

/**
 * SdkConfig -> <service>::Config for settings that come from generic smithy
 */
class GenericSmithySdkConfigSettings : ClientCodegenDecorator {
    override val name: String = "GenericSmithySdkConfigSettings"
    override val order: Byte = 0

    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> {
        val rc = codegenContext.runtimeConfig
        val codegenScope =
            arrayOf(
                *preludeScope,
                "AuthSchemePreference" to
                    RuntimeType.smithyRuntimeApiClient(rc)
                        .resolve("client::auth::AuthSchemePreference"),
                "RetryConfig" to RuntimeType.smithyTypes(rc).resolve("retry::RetryConfig"),
                "StalledStreamProtectionConfig" to
                    configReexport(RuntimeType.smithyRuntimeApi(rc).resolve("client::stalled_stream_protection::StalledStreamProtectionConfig")),
                "TimeoutConfig" to RuntimeType.smithyTypes(rc).resolve("timeout::TimeoutConfig"),
            )

        return listOf(
            adhocCustomization<ServiceConfigSection.MergeFromSharedConfig> { section ->
                rustTemplate(
                    """
                    if self.field_never_set::<#{RetryConfig}>() {
                        self.set_retry_config(${section.sdkConfig}.retry_config().cloned());
                    }

                    // The `set_timeout_config` method cannot be called here because it has a custom merge logic
                    // that assumes the shared config timeout is set first, followed by the service client timeout;
                    // the shared config timeout is stored in `CloneableLayer`, while the service client timeout is passed
                    // as an input to `set_timeout_config`. In this method, however, the shared config timeout resides
                    // in `sdk_config`, and the service client timeout is in `CloneableLayer`. Therefore, we need to
                    // manually replicate the merge logic to handle this different ordering of values.
                    match (
                        self.config
                            .load::<#{TimeoutConfig}>()
                            .cloned(),
                        sdk_config.timeout_config(),
                    ) {
                        (#{None}, #{Some}(shared_config_timeout)) => {
                            self.config.store_put(shared_config_timeout.clone());
                        }
                        (#{Some}(mut service_client_timeout), #{Some}(shared_config_timeout)) => {
                            service_client_timeout.take_defaults_from(shared_config_timeout);
                            self.config.store_put(service_client_timeout);
                        }
                        _ => {}
                    }

                    if self.runtime_components.sleep_impl().is_none() {
                        self.set_sleep_impl(${section.sdkConfig}.sleep_impl());
                    }
                    if self.runtime_components.http_client().is_none() {
                        self.set_http_client(${section.sdkConfig}.http_client());
                    }
                    if self.runtime_components.time_source().is_none() {
                        self.set_time_source(${section.sdkConfig}.time_source());
                    }
                    if self.behavior_version.is_none() {
                        self.set_behavior_version(${section.sdkConfig}.behavior_version());
                    }
                    if self.field_never_set::<#{AuthSchemePreference}>() {
                        self.set_auth_scheme_preference(${section.sdkConfig}.auth_scheme_preference().cloned());
                    }
                    if self.field_never_set::<#{StalledStreamProtectionConfig}>() {
                        // only call the setter if stall stream protection config is set in shared config,
                        // as setting `None` here removes the default
                        if let #{Some}(config) = ${section.sdkConfig}.stalled_stream_protection() {
                            self.set_stalled_stream_protection(#{Some}(config));
                        }
                    }
                    if self.runtime_components.identity_cache().is_none() {
                       if let #{Some}(cache) = ${section.sdkConfig}.identity_cache() {
                           self.set_identity_cache(cache);
                       }
                    }
                    """,
                    *codegenScope,
                )
            },
        )
    }
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
                    ##[allow(clippy::field_reassign_with_default)]
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
