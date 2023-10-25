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
        // TODO Docs
        writer.rustTemplate(
            """
            ##[derive(#{Debug})]
            pub struct ${serviceName}Config<L, H, M> {
                layers: L,
                http_plugins: H,
                model_plugins: M,
            }

            impl ${serviceName}Config<(), (), ()> {
                pub fn builder() -> config::Builder<
                    #{Tower}::layer::util::Identity,
                    #{SmithyHttpServer}::plugin::IdentityPlugin,
                    #{SmithyHttpServer}::plugin::IdentityPlugin,
                > {
                    config::Builder {
                        layers: #{Tower}::layer::util::Identity::new(),
                        http_plugins: #{SmithyHttpServer}::plugin::IdentityPlugin,
                        model_plugins: #{SmithyHttpServer}::plugin::IdentityPlugin,
                    }
                }
            }

            /// Module hosting the builder for [`${serviceName}Config`].
            pub mod config {
                ##[derive(#{Debug})]
                pub struct Builder<L, H, M> {
                    pub(crate) layers: L,
                    pub(crate) http_plugins: H,
                    pub(crate) model_plugins: M,
                }

                impl<L, H, M> Builder<L, H, M> {
                    pub fn layer<NewLayer>(self, layer: NewLayer) -> Builder<#{Stack}<NewLayer, L>, H, M> {
                        Builder {
                            layers: #{Stack}::new(layer, self.layers),
                            http_plugins: self.http_plugins,
                            model_plugins: self.model_plugins,
                        }
                    }

                    // We eagerly require `NewPlugin: HttpMarker`, despite not really needing it, because compiler
                    // errors get _substantially_ better if the user makes a mistake.
                    pub fn http_plugin<NewPlugin: #{HttpMarker}>(
                        self,
                        http_plugin: NewPlugin,
                    ) -> Builder<L, #{PluginStack}<NewPlugin, H>, M> {
                        Builder {
                            layers: self.layers,
                            http_plugins: #{PluginStack}::new(http_plugin, self.http_plugins),
                            model_plugins: self.model_plugins,
                        }
                    }

                    // We eagerly require `NewPlugin: ModelMarker`, despite not really needing it, because compiler
                    // errors get _substantially_ better if the user makes a mistake.
                    pub fn model_plugin<NewPlugin: #{ModelMarker}>(
                        self,
                        model_plugin: NewPlugin,
                    ) -> Builder<L, H, #{PluginStack}<NewPlugin, M>> {
                        Builder {
                            layers: self.layers,
                            http_plugins: self.http_plugins,
                            model_plugins: #{PluginStack}::new(model_plugin, self.model_plugins),
                        }
                    }

                    pub fn build(self) -> super::${serviceName}Config<L, H, M> {
                        super::${serviceName}Config {
                            layers: self.layers,
                            http_plugins: self.http_plugins,
                            model_plugins: self.model_plugins,
                        }
                    }
                }
            }
            """,
            *codegenScope,
        )
    }
}
