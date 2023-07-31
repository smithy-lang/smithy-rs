/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.util.letIf

class HttpConnectorConfigDecorator : ClientCodegenDecorator {
    override val name: String = "HttpConnectorConfigDecorator"
    override val order: Byte = 0

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> =
        baseCustomizations.letIf(codegenContext.smithyRuntimeMode.generateOrchestrator) {
            it + HttpConnectorConfigCustomization(codegenContext)
        }
}

private class HttpConnectorConfigCustomization(
    codegenContext: ClientCodegenContext,
) : ConfigCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val runtimeMode = codegenContext.smithyRuntimeMode
    private val moduleUseName = codegenContext.moduleUseName()
    private val codegenScope = arrayOf(
        *preludeScope,
        "Connection" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("client::orchestrator::Connection"),
        "ConnectorSettings" to RuntimeType.smithyClient(runtimeConfig).resolve("http_connector::ConnectorSettings"),
        "DynConnectorAdapter" to RuntimeType.smithyRuntime(runtimeConfig).resolve("client::connectors::adapter::DynConnectorAdapter"),
        "HttpConnector" to RuntimeType.smithyClient(runtimeConfig).resolve("http_connector::HttpConnector"),
        "Resolver" to RuntimeType.smithyRuntime(runtimeConfig).resolve("client::config_override::Resolver"),
        "SharedAsyncSleep" to RuntimeType.smithyAsync(runtimeConfig).resolve("rt::sleep::SharedAsyncSleep"),
        "SharedHttpConnector" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("client::connectors::SharedHttpConnector"),
        "TimeoutConfig" to RuntimeType.smithyTypes(runtimeConfig).resolve("timeout::TimeoutConfig"),
    )

    private fun defaultConnectorFn(): RuntimeType = RuntimeType.forInlineFun("default_connector", ClientRustModule.config) {
        rustTemplate(
            """
            ##[cfg(feature = "rustls")]
            fn default_connector(
                connector_settings: &#{ConnectorSettings},
                sleep_impl: #{Option}<#{SharedAsyncSleep}>,
            ) -> #{Option}<#{DynConnector}> {
                #{default_connector}(connector_settings, sleep_impl)
            }

            ##[cfg(not(feature = "rustls"))]
            fn default_connector(
                _connector_settings: &#{ConnectorSettings},
                _sleep_impl: #{Option}<#{SharedAsyncSleep}>,
            ) -> #{Option}<#{DynConnector}> {
                #{None}
            }
            """,
            *codegenScope,
            "default_connector" to RuntimeType.smithyClient(runtimeConfig).resolve("conns::default_connector"),
            "DynConnector" to RuntimeType.smithyClient(runtimeConfig).resolve("erase::DynConnector"),
        )
    }

    private fun setConnectorFn(): RuntimeType = RuntimeType.forInlineFun("set_connector", ClientRustModule.config) {
        rustTemplate(
            """
            fn set_connector(resolver: &mut #{Resolver}<'_>) {
                // Initial configuration needs to set a default if no connector is given, so it
                // should always get into the condition below.
                //
                // Override configuration should set the connector if the override config
                // contains a connector, sleep impl, or a timeout config since these are all
                // incorporated into the final connector.
                let must_set_connector = resolver.is_initial()
                    || resolver.is_latest_set::<#{HttpConnector}>()
                    || resolver.latest_sleep_impl().is_some()
                    || resolver.is_latest_set::<#{TimeoutConfig}>();
                if must_set_connector {
                    let sleep_impl = resolver.sleep_impl();
                    let timeout_config = resolver.resolve_config::<#{TimeoutConfig}>()
                        .cloned()
                        .unwrap_or_else(#{TimeoutConfig}::disabled);
                    let connector_settings = #{ConnectorSettings}::from_timeout_config(&timeout_config);
                    let http_connector = resolver.resolve_config::<#{HttpConnector}>();

                    // TODO(enableNewSmithyRuntimeCleanup): Replace the tower-based DynConnector and remove DynConnectorAdapter when deleting the middleware implementation
                    let connector =
                        http_connector
                            .and_then(|c| c.connector(&connector_settings, sleep_impl.clone()))
                            .or_else(|| #{default_connector}(&connector_settings, sleep_impl))
                            .map(|c| #{SharedHttpConnector}::new(#{DynConnectorAdapter}::new(c)));

                    resolver.runtime_components_mut().set_http_connector(connector);
                }
            }
            """,
            *codegenScope,
            "default_connector" to defaultConnectorFn(),
        )
    }

    override fun section(section: ServiceConfig): Writable {
        return when (section) {
            is ServiceConfig.ConfigStruct -> writable {
                if (runtimeMode.generateMiddleware) {
                    rustTemplate("http_connector: Option<#{HttpConnector}>,", *codegenScope)
                }
            }

            is ServiceConfig.ConfigImpl -> writable {
                if (runtimeMode.generateOrchestrator) {
                    rustTemplate(
                        """
                        /// Return the [`SharedHttpConnector`](#{SharedHttpConnector}) to use when making requests, if any.
                        pub fn http_connector(&self) -> Option<#{SharedHttpConnector}> {
                            self.runtime_components.http_connector()
                        }
                        """,
                        *codegenScope,
                    )
                } else {
                    rustTemplate(
                        """
                        /// Return an [`HttpConnector`](#{HttpConnector}) to use when making requests, if any.
                        pub fn http_connector(&self) -> Option<&#{HttpConnector}> {
                            self.http_connector.as_ref()
                        }
                        """,
                        *codegenScope,
                    )
                }
            }

            is ServiceConfig.BuilderStruct -> writable {
                if (runtimeMode.generateMiddleware) {
                    rustTemplate("http_connector: Option<#{HttpConnector}>,", *codegenScope)
                }
            }

            ServiceConfig.BuilderImpl -> writable {
                rustTemplate(
                    """
                    /// Sets the HTTP connector to use when making requests.
                    ///
                    /// ## Examples
                    /// ```no_run
                    /// ## ##[cfg(test)]
                    /// ## mod tests {
                    /// ## ##[test]
                    /// ## fn example() {
                    /// use std::time::Duration;
                    /// use aws_smithy_client::{Client, hyper_ext};
                    /// use aws_smithy_client::erase::DynConnector;
                    /// use aws_smithy_client::http_connector::ConnectorSettings;
                    /// use $moduleUseName::config::Config;
                    ///
                    /// let https_connector = hyper_rustls::HttpsConnectorBuilder::new()
                    ///     .with_webpki_roots()
                    ///     .https_only()
                    ///     .enable_http1()
                    ///     .enable_http2()
                    ///     .build();
                    /// let smithy_connector = hyper_ext::Adapter::builder()
                    ///     // Optionally set things like timeouts as well
                    ///     .connector_settings(
                    ///         ConnectorSettings::builder()
                    ///             .connect_timeout(Duration::from_secs(5))
                    ///             .build()
                    ///     )
                    ///     .build(https_connector);
                    /// ## }
                    /// ## }
                    /// ```
                    pub fn http_connector(mut self, http_connector: impl Into<#{HttpConnector}>) -> Self {
                        self.set_http_connector(#{Some}(http_connector));
                        self
                    }

                    /// Sets the HTTP connector to use when making requests.
                    ///
                    /// ## Examples
                    /// ```no_run
                    /// ## ##[cfg(test)]
                    /// ## mod tests {
                    /// ## ##[test]
                    /// ## fn example() {
                    /// use std::time::Duration;
                    /// use aws_smithy_client::hyper_ext;
                    /// use aws_smithy_client::http_connector::ConnectorSettings;
                    /// use $moduleUseName::config::{Builder, Config};
                    ///
                    /// fn override_http_connector(builder: &mut Builder) {
                    ///     let https_connector = hyper_rustls::HttpsConnectorBuilder::new()
                    ///         .with_webpki_roots()
                    ///         .https_only()
                    ///         .enable_http1()
                    ///         .enable_http2()
                    ///         .build();
                    ///     let smithy_connector = hyper_ext::Adapter::builder()
                    ///         // Optionally set things like timeouts as well
                    ///         .connector_settings(
                    ///             ConnectorSettings::builder()
                    ///                 .connect_timeout(Duration::from_secs(5))
                    ///                 .build()
                    ///         )
                    ///         .build(https_connector);
                    ///     builder.set_http_connector(Some(smithy_connector));
                    /// }
                    ///
                    /// let mut builder = $moduleUseName::Config::builder();
                    /// override_http_connector(&mut builder);
                    /// let config = builder.build();
                    /// ## }
                    /// ## }
                    /// ```
                    """,
                    *codegenScope,
                )
                if (runtimeMode.generateOrchestrator) {
                    rustTemplate(
                        """
                        pub fn set_http_connector(&mut self, http_connector: Option<impl Into<#{HttpConnector}>>) -> &mut Self {
                            http_connector.map(|c| self.config.store_put(c.into()));
                            self
                        }
                        """,
                        *codegenScope,
                    )
                } else {
                    rustTemplate(
                        """
                        pub fn set_http_connector(&mut self, http_connector: Option<impl Into<#{HttpConnector}>>) -> &mut Self {
                            self.http_connector = http_connector.map(|inner| inner.into());
                            self
                        }
                        """,
                        *codegenScope,
                    )
                }
            }

            is ServiceConfig.BuilderBuild -> writable {
                if (runtimeMode.generateOrchestrator) {
                    rustTemplate(
                        "#{set_connector}(&mut resolver);",
                        "set_connector" to setConnectorFn(),
                    )
                } else {
                    rust("http_connector: self.http_connector,")
                }
            }

            is ServiceConfig.OperationConfigOverride -> writable {
                if (runtimeMode.generateOrchestrator) {
                    rustTemplate(
                        "#{set_connector}(&mut resolver);",
                        "set_connector" to setConnectorFn(),
                    )
                }
            }

            else -> emptySection
        }
    }
}
