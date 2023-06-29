/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.smithyRuntime
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.smithyRuntimeApi
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.smithyTypes
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.util.letIf

class ConnectionPoisoningDecorator : ClientCodegenDecorator {
    override val name: String get() = "ConnectionPoisoningDecorator"
    override val order: Byte get() = 0

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> =
        baseCustomizations.letIf(codegenContext.smithyRuntimeMode.generateOrchestrator) {
            it + listOf(ConnectionPoisoningRuntimePluginCustomization(codegenContext))
        }

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> =
        baseCustomizations.letIf(codegenContext.smithyRuntimeMode.generateOrchestrator) {
            it + listOf(ConnectionPoisoningConfigCustomization(codegenContext))
        }

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        val runtimeConfig = codegenContext.runtimeConfig
        if (codegenContext.smithyRuntimeMode.generateOrchestrator) {
            rustCrate.withModule(ClientRustModule.config) {
                rustTemplate(
                    "pub use #{ReconnectMode};",
                    "ReconnectMode" to smithyTypes(runtimeConfig).resolve("retry::ReconnectMode"),
                )
            }
        }
    }
}

private class ConnectionPoisoningRuntimePluginCustomization(
    codegenContext: ClientCodegenContext,
) : ServiceRuntimePluginCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope = arrayOf(
        *preludeScope,
        "ReconnectMode" to smithyTypes(runtimeConfig).resolve("retry::ReconnectMode"),
        "ConnectionPoisoningInterceptor" to smithyRuntime(runtimeConfig).resolve("client::connectors::connection_poisoning::ConnectionPoisoningInterceptor"),
    )

    override fun section(section: ServiceRuntimePluginSection): Writable = writable {
        when (section) {
            is ServiceRuntimePluginSection.RegisterInterceptor -> {
                // This interceptor assumes that a compatible Connector is set. Otherwise, connection poisoning
                // won't work and an error message will be logged.
                section.registerInterceptor(runtimeConfig, this) {
                    rustTemplate("#{ConnectionPoisoningInterceptor}::new()", *codegenScope)
                }
            }

            else -> emptySection
        }
    }
}

private class ConnectionPoisoningConfigCustomization(codegenContext: ClientCodegenContext) : ConfigCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope = arrayOf(
        *preludeScope,
        "DynRetryStrategy" to smithyRuntimeApi(runtimeConfig).resolve("client::retries::DynRetryStrategy"),
        "ReconnectMode" to smithyTypes(runtimeConfig).resolve("retry::ReconnectMode"),
        "ConnectionPoisoningInterceptor" to smithyRuntime(runtimeConfig).resolve("client::connectors::connection_poisoning::ConnectionPoisoningInterceptor"),
    )

    override fun section(section: ServiceConfig) =
        writable {
            when (section) {
                is ServiceConfig.ConfigImpl -> {
                    rustTemplate(
                        """
                        /// Mode for connection re-establishment
                        ///
                        /// By default, when a transient error is encountered, the connection in use will be poisoned. This
                        /// behavior can be disabled by setting [`ReconnectMode::ReuseAllConnections`] instead.
                        pub fn reconnect_mode(&self) -> #{Option}<#{ReconnectMode}> {
                            self.inner.load::<#{ReconnectMode}>().cloned()
                        }
                        """,
                        *codegenScope,
                    )
                }

                is ServiceConfig.BuilderImpl -> {
                    rustTemplate(
                        """
                        /// Set the reconnect_mode for the builder
                        pub fn reconnect_mode(mut self, reconnect_mode: #{ReconnectMode}) -> Self {
                            self.set_reconnect_mode(Some(reconnect_mode));
                            self
                        }

                        /// Set the reconnect_mode for the builder
                        pub fn set_reconnect_mode(&mut self, reconnect_mode: #{Option}<#{ReconnectMode}>) -> &mut Self {
                            reconnect_mode.map(|s| self.inner.store_put(s));
                            self
                        }
                        """,
                        *codegenScope,
                    )
                }

                else -> emptySection
            }
        }
}
