/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext

class ServiceConfigGenerator(
    codegenContext: ServerCodegenContext,
) {
    private val crateName = codegenContext.moduleUseName()
    private val codegenScope = codegenContext.runtimeConfig.let { runtimeConfig ->
        val smithyHttpServer = ServerCargoDependency.smithyHttpServer(runtimeConfig).toType()
        arrayOf(
            "Debug" to RuntimeType.Debug,
            "SmithyHttpServer" to smithyHttpServer,
            "PluginStack" to smithyHttpServer.resolve("plugin::PluginStack"),
            "ModelMarker" to smithyHttpServer.resolve("plugin::ModelMarker"),
            "HttpMarker" to smithyHttpServer.resolve("plugin::HttpMarker"),
            "Tower" to RuntimeType.Tower,
            "Stack" to RuntimeType.Tower.resolve("layer::util::Stack"),
        )
    }
    private val serviceName = codegenContext.serviceShape.id.name.toPascalCase()

    fun render(writer: RustWriter) {
        writer.rustTemplate(
            """
            /// Configuration for the [`$serviceName`]. This is the central place where to register and
            /// configure [`#{Tower}::Layer`]s, HTTP plugins, and model plugins.
            ///
            /// ```rust,no_run
            /// ## use $crateName::${serviceName}Config;
            /// ## use #{SmithyHttpServer}::plugin::IdentityPlugin;
            /// ## use #{Tower}::layer::util::Identity;
            /// ## let authentication_plugin = IdentityPlugin;
            /// ## let authorization_plugin = IdentityPlugin;
            /// ## let server_request_id_provider_layer = Identity::new();
            /// let config = ${serviceName}Config::builder()
            ///     // Layers get executed first...
            ///     .layer(server_request_id_provider_layer)
            ///     // ...then HTTP plugins...
            ///     .http_plugin(authentication_plugin)
            ///     // ...and right after deserialization, model plugins.
            ///     .model_plugin(authorization_plugin)
            ///     .build();
            /// ```
            ///
            /// See the [`plugin`] system for details.
            ///
            /// [`plugin`]: #{SmithyHttpServer}::plugin
            ##[derive(#{Debug})]
            pub struct ${serviceName}Config<L, H, M> {
                layers: L,
                http_plugins: H,
                model_plugins: M,
            }

            impl ${serviceName}Config<(), (), ()> {
                /// Returns a builder to construct the configuration.
                pub fn builder() -> ${serviceName}ConfigBuilder<
                    #{Tower}::layer::util::Identity,
                    #{SmithyHttpServer}::plugin::IdentityPlugin,
                    #{SmithyHttpServer}::plugin::IdentityPlugin,
                > {
                    ${serviceName}ConfigBuilder {
                        layers: #{Tower}::layer::util::Identity::new(),
                        http_plugins: #{SmithyHttpServer}::plugin::IdentityPlugin,
                        model_plugins: #{SmithyHttpServer}::plugin::IdentityPlugin,
                    }
                }
            }

            /// Builder returned by [`${serviceName}Config::builder()`].
            ##[derive(#{Debug})]
            pub struct ${serviceName}ConfigBuilder<L, H, M> {
                pub(crate) layers: L,
                pub(crate) http_plugins: H,
                pub(crate) model_plugins: M,
            }

            impl<L, H, M> ${serviceName}ConfigBuilder<L, H, M> {
                /// Add a [`#{Tower}::Layer`] to the service.
                pub fn layer<NewLayer>(self, layer: NewLayer) -> ${serviceName}ConfigBuilder<#{Stack}<NewLayer, L>, H, M> {
                    ${serviceName}ConfigBuilder {
                        layers: #{Stack}::new(layer, self.layers),
                        http_plugins: self.http_plugins,
                        model_plugins: self.model_plugins,
                    }
                }

                /// Add a HTTP [plugin] to the service.
                ///
                /// [plugin]: #{SmithyHttpServer}::plugin
                // We eagerly require `NewPlugin: HttpMarker`, despite not really needing it, because compiler
                // errors get _substantially_ better if the user makes a mistake.
                pub fn http_plugin<NewPlugin: #{HttpMarker}>(
                    self,
                    http_plugin: NewPlugin,
                ) -> ${serviceName}ConfigBuilder<L, #{PluginStack}<NewPlugin, H>, M> {
                    ${serviceName}ConfigBuilder {
                        layers: self.layers,
                        http_plugins: #{PluginStack}::new(http_plugin, self.http_plugins),
                        model_plugins: self.model_plugins,
                    }
                }

                /// Add a model [plugin] to the service.
                ///
                /// [plugin]: #{SmithyHttpServer}::plugin
                // We eagerly require `NewPlugin: ModelMarker`, despite not really needing it, because compiler
                // errors get _substantially_ better if the user makes a mistake.
                pub fn model_plugin<NewPlugin: #{ModelMarker}>(
                    self,
                    model_plugin: NewPlugin,
                ) -> ${serviceName}ConfigBuilder<L, H, #{PluginStack}<NewPlugin, M>> {
                    ${serviceName}ConfigBuilder {
                        layers: self.layers,
                        http_plugins: self.http_plugins,
                        model_plugins: #{PluginStack}::new(model_plugin, self.model_plugins),
                    }
                }

                /// Build the configuration.
                pub fn build(self) -> super::${serviceName}Config<L, H, M> {
                    super::${serviceName}Config {
                        layers: self.layers,
                        http_plugins: self.http_plugins,
                        model_plugins: self.model_plugins,
                    }
                }
            }
            """,
            *codegenScope,
        )
    }
}
