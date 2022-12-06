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
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

class HttpConnectorDecorator : RustCodegenDecorator<ClientProtocolGenerator, ClientCodegenContext> {
    override val name: String = "HttpConnectorDecorator"
    override val order: Byte = 0

    override fun supportsCodegenContext(clazz: Class<out CodegenContext>): Boolean =
        clazz.isAssignableFrom(ClientCodegenContext::class.java)

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations + HttpConnectorConfigCustomization(codegenContext)
    }
}

class HttpConnectorConfigCustomization(
    codegenContext: CodegenContext,
) : ConfigCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val moduleUseName = codegenContext.moduleUseName()
    private val codegenScope = arrayOf(
        "HttpConnector" to RuntimeType.smithyClient(runtimeConfig).resolve("http_connector::HttpConnector"),
    )

    override fun section(section: ServiceConfig): Writable {
        return when (section) {
            is ServiceConfig.ConfigStruct -> writable {
                rustTemplate("http_connector: Option<#{HttpConnector}>,", *codegenScope)
            }
            is ServiceConfig.ConfigImpl -> writable {
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
            is ServiceConfig.BuilderStruct -> writable {
                rustTemplate("http_connector: Option<#{HttpConnector}>,", *codegenScope)
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
                        self.http_connector = Some(http_connector.into());
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
                    /// use crate::sdk_config::{SdkConfig, Builder};
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
                    pub fn set_http_connector(&mut self, http_connector: Option<impl Into<#{HttpConnector}>>) -> &mut Self {
                        self.http_connector = http_connector.map(|inner| inner.into());
                        self
                    }
                    """,
                    *codegenScope,
                )
            }
            is ServiceConfig.BuilderBuild -> writable {
                rust("http_connector: self.http_connector,")
            }
            else -> emptySection
        }
    }
}
