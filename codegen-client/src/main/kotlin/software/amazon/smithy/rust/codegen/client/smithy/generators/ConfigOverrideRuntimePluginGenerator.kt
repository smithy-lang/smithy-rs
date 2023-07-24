/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations

class ConfigOverrideRuntimePluginGenerator(
    codegenContext: ClientCodegenContext,
) {
    private val moduleUseName = codegenContext.moduleUseName()
    private val codegenScope = codegenContext.runtimeConfig.let { rc ->
        val runtimeApi = RuntimeType.smithyRuntimeApi(rc)
        val smithyTypes = RuntimeType.smithyTypes(rc)
        arrayOf(
            *RuntimeType.preludeScope,
            "Cow" to RuntimeType.Cow,
            "CloneableLayer" to smithyTypes.resolve("config_bag::CloneableLayer"),
            "FrozenLayer" to smithyTypes.resolve("config_bag::FrozenLayer"),
            "InterceptorRegistrar" to runtimeApi.resolve("client::interceptors::InterceptorRegistrar"),
            "Layer" to smithyTypes.resolve("config_bag::Layer"),
            "Resolver" to RuntimeType.smithyRuntime(rc).resolve("client::config_override::Resolver"),
            "RuntimeComponentsBuilder" to RuntimeType.runtimeComponentsBuilder(rc),
            "RuntimePlugin" to RuntimeType.runtimePlugin(rc),
        )
    }

    fun render(writer: RustWriter, customizations: List<ConfigCustomization>) {
        writer.rustTemplate(
            """
            /// A plugin that enables configuration for a single operation invocation
            ///
            /// The `config` method will return a `FrozenLayer` by storing values from `config_override`.
            /// In the case of default values requested, they will be obtained from `client_config`.
            ##[derive(Debug)]
            pub(crate) struct ConfigOverrideRuntimePlugin {
                pub(crate) config: #{FrozenLayer},
                pub(crate) components: #{RuntimeComponentsBuilder},
            }

            impl ConfigOverrideRuntimePlugin {
                pub(crate) fn new(
                    config_override: Builder,
                    initial_config: #{FrozenLayer},
                    initial_components: &#{RuntimeComponentsBuilder}
                ) -> Self {
                    let mut layer = config_override.config;
                    let mut components = config_override.runtime_components;
                    let mut resolver = #{Resolver}::overrid(initial_config, initial_components, &mut layer, &mut components);

                    #{config}

                    let _ = resolver;
                    Self {
                        config: #{Layer}::from(layer)
                            .with_name("$moduleUseName::config::ConfigOverrideRuntimePlugin").freeze(),
                        components,
                    }
                }
            }

            impl #{RuntimePlugin} for ConfigOverrideRuntimePlugin {
                fn config(&self) -> #{Option}<#{FrozenLayer}> {
                    Some(self.config.clone())
                }

                fn runtime_components(&self) -> #{Cow}<'_, #{RuntimeComponentsBuilder}> {
                    #{Cow}::Borrowed(&self.components)
                }
            }
            """,
            *codegenScope,
            "config" to writable {
                writeCustomizations(
                    customizations,
                    ServiceConfig.OperationConfigOverride("layer"),
                )
            },
        )
    }
}
