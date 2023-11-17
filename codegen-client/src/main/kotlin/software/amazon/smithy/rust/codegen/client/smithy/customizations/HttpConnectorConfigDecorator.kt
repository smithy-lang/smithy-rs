/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.configReexport
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope

class HttpConnectorConfigDecorator : ClientCodegenDecorator {
    override val name: String = "HttpConnectorConfigDecorator"
    override val order: Byte = 0

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> = baseCustomizations + HttpConnectorConfigCustomization(codegenContext)
}

private class HttpConnectorConfigCustomization(
    codegenContext: ClientCodegenContext,
) : ConfigCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val moduleUseName = codegenContext.moduleUseName()
    private val codegenScope = arrayOf(
        *preludeScope,
        "HttpClient" to configReexport(
            RuntimeType.smithyRuntimeApiClient(runtimeConfig).resolve("client::http::HttpClient"),
        ),
        "IntoShared" to configReexport(RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("shared::IntoShared")),
        "SharedHttpClient" to configReexport(
            RuntimeType.smithyRuntimeApiClient(runtimeConfig).resolve("client::http::SharedHttpClient"),
        ),
    )

    override fun section(section: ServiceConfig): Writable {
        return when (section) {
            is ServiceConfig.ConfigImpl -> writable {
                rustTemplate(
                    """
                    /// Return the [`SharedHttpClient`](#{SharedHttpClient}) to use when making requests, if any.
                    pub fn http_client(&self) -> Option<#{SharedHttpClient}> {
                        self.runtime_components.http_client()
                    }
                    """,
                    *codegenScope,
                )
            }

            ServiceConfig.BuilderImpl -> writable {
                rustTemplate(
                    """
                    /// Sets the HTTP client to use when making requests.
                    ///
                    /// ## Examples
                    /// ```no_run
                    /// ## ##[cfg(test)]
                    /// ## mod tests {
                    /// ## ##[test]
                    /// ## fn example() {
                    /// use std::time::Duration;
                    /// use $moduleUseName::config::Config;
                    /// use aws_smithy_runtime::client::http::hyper_014::HyperClientBuilder;
                    ///
                    /// let https_connector = hyper_rustls::HttpsConnectorBuilder::new()
                    ///     .with_webpki_roots()
                    ///     .https_only()
                    ///     .enable_http1()
                    ///     .enable_http2()
                    ///     .build();
                    /// let hyper_client = HyperClientBuilder::new().build(https_connector);
                    ///
                    /// // This connector can then be given to a generated service Config
                    /// let config = my_service_client::Config::builder()
                    ///     .endpoint_url("https://example.com")
                    ///     .http_client(hyper_client)
                    ///     .build();
                    /// let client = my_service_client::Client::from_conf(config);
                    /// ## }
                    /// ## }
                    /// ```
                    pub fn http_client(mut self, http_client: impl #{HttpClient} + 'static) -> Self {
                        self.set_http_client(#{Some}(#{IntoShared}::into_shared(http_client)));
                        self
                    }

                    /// Sets the HTTP client to use when making requests.
                    ///
                    /// ## Examples
                    /// ```no_run
                    /// ## ##[cfg(test)]
                    /// ## mod tests {
                    /// ## ##[test]
                    /// ## fn example() {
                    /// use std::time::Duration;
                    /// use $moduleUseName::config::{Builder, Config};
                    /// use aws_smithy_runtime::client::http::hyper_014::HyperClientBuilder;
                    ///
                    /// fn override_http_client(builder: &mut Builder) {
                    ///     let https_connector = hyper_rustls::HttpsConnectorBuilder::new()
                    ///         .with_webpki_roots()
                    ///         .https_only()
                    ///         .enable_http1()
                    ///         .enable_http2()
                    ///         .build();
                    ///     let hyper_client = HyperClientBuilder::new().build(https_connector);
                    ///     builder.set_http_client(Some(hyper_client));
                    /// }
                    ///
                    /// let mut builder = $moduleUseName::Config::builder();
                    /// override_http_client(&mut builder);
                    /// let config = builder.build();
                    /// ## }
                    /// ## }
                    /// ```
                    pub fn set_http_client(&mut self, http_client: Option<#{SharedHttpClient}>) -> &mut Self {
                        self.runtime_components.set_http_client(http_client);
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
