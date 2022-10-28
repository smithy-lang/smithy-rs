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
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.SmithyClient
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.asType
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext

class HttpConnectorDecorator : RustCodegenDecorator<ClientProtocolGenerator, ClientCodegenContext> {
    override val name: String = "HttpConnectorDecorator"
    override val order: Byte = 0

    override fun supportsCodegenContext(clazz: Class<out CodegenContext>): Boolean =
        clazz.isAssignableFrom(ClientCodegenContext::class.java)

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>
    ): List<ConfigCustomization> {
        return baseCustomizations + HttpConnectorConfigCustomization(codegenContext)
    }
}

class HttpConnectorConfigCustomization(
    codegenContext: CodegenContext
) : ConfigCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val moduleUseName = codegenContext.moduleUseName()
    private val codegenScope = arrayOf(
        "HttpConnector" to SmithyClient(runtimeConfig).asType().member("http_connector::HttpConnector")
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
                    *codegenScope
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
                    /// ```
                    /// ## ##[cfg(test)]
                    /// ## mod tests {
                    /// ## ##[test]
                    /// ## fn example() {
                    /// use aws_smithy_client::test_connection::TestConnection;
                    /// use $moduleUseName::config::Config;
                    ///
                    /// let config = $moduleUseName::Config::builder()
                    ///     .http_connector(TestConnection::new(vec![]).into())
                    ///     .build();
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
                    /// ```
                    /// ## ##[cfg(test)]
                    /// ## mod tests {
                    /// ## ##[test]
                    /// ## fn example() {
                    /// use aws_smithy_client::test_connection::TestConnection;
                    /// use $moduleUseName::config::{Builder, Config};
                    ///
                    /// fn override_http_connector(builder: &mut Builder) {
                    ///     builder.set_http_connector(TestConnection::new(vec![]).into());
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
