/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate

class RuntimePluginConfigCustomization(codegenContext: CodegenContext) : ConfigCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val runtimePluginModule = RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("runtime_plugin")
    private val codegenScope = arrayOf(
        "RuntimePlugin" to runtimePluginModule.resolve("RuntimePlugin"),
        "SharedRuntimePlugin" to runtimePluginModule.resolve("SharedRuntimePlugin"),
    )

    override fun section(section: ServiceConfig) =
        writable {
            when (section) {
                is ServiceConfig.ConfigStruct -> rustTemplate(
                    """
                    // TODO(RuntimePlugins): Unused until integrated with [`invoke`](aws_smithy_runtime::invoke).
                    ##[allow(dead_code)]
                    pub(crate) client_plugins: Vec<#{SharedRuntimePlugin}>,
                    """,
                    *codegenScope,
                )

                is ServiceConfig.ConfigImpl -> {}

                is ServiceConfig.BuilderStruct ->
                    rustTemplate(
                        """
                        client_plugins: Vec<#{SharedRuntimePlugin}>,
                        """,
                        *codegenScope,
                    )

                ServiceConfig.BuilderImpl ->
                    rustTemplate(
                        """
                        /// Set the `runtime_plugin` for the builder
                        ///
                        /// `runtime_plugin` is applied to the service configuration level.
                        ///
                        /// ## Examples
                        /// ```no_run
                        /// use aws_smithy_runtime_api::config_bag::ConfigBag;
                        /// use aws_smithy_runtime_api::runtime_plugin::{BoxError, RuntimePlugin};
                        /// use aws_types::sdk_config::{SdkConfig, Builder};
                        ///
                        /// ##[derive(Debug)]
                        /// struct APlugin;
                        ///
                        /// impl RuntimePlugin for APlugin {
                        ///     fn configure(&self, _cfg: &mut ConfigBag) -> Result<(), BoxError> {
                        ///         // ..
                        ///         ## todo!()
                        ///     }
                        /// }
                        ///
                        /// SdkConfig::builder().runtime_plugin(APlugin{}).build();
                        /// ```
                        pub fn runtime_plugin(mut self, runtime_plugin: impl #{RuntimePlugin} + 'static) -> Self {
                            self.set_runtime_plugin(runtime_plugin);
                            self
                        }

                        /// Set the `runtime_plugin` for the builder
                        ///
                        /// `runtime_plugin` is applied to the service configuration level.
                        ///
                        /// ## Examples
                        /// ```no_run
                        /// use aws_smithy_runtime_api::config_bag::ConfigBag;
                        /// use aws_smithy_runtime_api::runtime_plugin::{BoxError, RuntimePlugin};
                        /// use aws_types::sdk_config::{SdkConfig, Builder};
                        ///
                        /// ##[derive(Debug)]
                        /// struct APlugin;
                        ///
                        /// impl RuntimePlugin for APlugin {
                        ///     fn configure(&self, _cfg: &mut ConfigBag) -> Result<(), BoxError> {
                        ///         // ..
                        ///         ## todo!()
                        ///     }
                        /// }
                        ///
                        /// fn set_runtime_plugin(builder: &mut Builder) {
                        ///     builder.set_runtime_plugin(APlugin{});
                        /// }
                        ///
                        /// let mut builder = SdkConfig::builder();
                        /// set_runtime_plugin(&mut builder);
                        /// builder.build();
                        /// ```
                        pub fn set_runtime_plugin(
                            &mut self,
                            runtime_plugin: impl #{RuntimePlugin} + 'static,
                        ) -> &mut Self {
                            self.client_plugins
                                .push(#{SharedRuntimePlugin}::new(runtime_plugin));
                            self
                        }

                        /// Bulk set [`SharedRuntimePlugin`](aws_smithy_runtime_api::runtime_plugin::SharedRuntimePlugin)s
                        /// for the builder
                        ///
                        /// `runtime_plugins` are applied to the service configuration level.
                        ///
                        /// ## Examples
                        /// ```no_run
                        /// use aws_smithy_runtime_api::config_bag::ConfigBag;
                        /// use aws_smithy_runtime_api::runtime_plugin::{BoxError, RuntimePlugin, SharedRuntimePlugin};
                        /// use aws_types::sdk_config::{SdkConfig, Builder};
                        ///
                        /// ##[derive(Debug)]
                        /// struct APlugin;
                        ///
                        /// impl RuntimePlugin for APlugin {
                        ///     fn configure(&self, _cfg: &mut ConfigBag) -> Result<(), BoxError> {
                        ///         // ..
                        ///         ## todo!()
                        ///     }
                        /// }
                        ///
                        /// let plugins = vec![
                        ///     SharedRuntimePlugin::new(APlugin{}),
                        ///     SharedRuntimePlugin::new(APlugin{}),
                        ///     SharedRuntimePlugin::new(APlugin{}),
                        /// ];
                        /// SdkConfig::builder().runtime_plugins(plugins).build();
                        /// ```
                        pub fn runtime_plugins(
                            mut self,
                            runtime_plugins: impl IntoIterator<Item = #{SharedRuntimePlugin}>,
                        ) -> Self {
                            self.set_runtime_plugins(runtime_plugins);
                            self
                        }

                        /// Bulk set [`SharedRuntimePlugin`](aws_smithy_runtime_api::runtime_plugin::SharedRuntimePlugin)s
                        /// for the builder
                        ///
                        /// `runtime_plugins` are applied to the service configuration level.
                        ///
                        /// ## Examples
                        /// ```no_run
                        /// use aws_smithy_runtime_api::config_bag::ConfigBag;
                        /// use aws_smithy_runtime_api::runtime_plugin::{BoxError, RuntimePlugin, SharedRuntimePlugin};
                        /// use aws_types::sdk_config::{SdkConfig, Builder};
                        ///
                        /// ##[derive(Debug)]
                        /// struct APlugin;
                        ///
                        /// impl RuntimePlugin for APlugin {
                        ///     fn configure(&self, _cfg: &mut ConfigBag) -> Result<(), BoxError> {
                        ///         // ..
                        ///         ## todo!()
                        ///     }
                        /// }
                        ///
                        /// fn set_runtime_plugins(builder: &mut Builder) {
                        ///     let plugins = vec![
                        ///         SharedRuntimePlugin::new(APlugin{}),
                        ///         SharedRuntimePlugin::new(APlugin{}),
                        ///         SharedRuntimePlugin::new(APlugin{}),
                        ///     ];
                        ///     builder.set_runtime_plugins(plugins);
                        /// }
                        ///
                        /// let mut builder = SdkConfig::builder();
                        /// set_runtime_plugins(&mut builder);
                        /// builder.build();
                        /// ```
                        pub fn set_runtime_plugins(
                            &mut self,
                            runtime_plugins: impl IntoIterator<Item = #{SharedRuntimePlugin}>,
                        ) -> &mut Self {
                            self.client_plugins.extend(runtime_plugins.into_iter());
                            self
                        }
                        """,
                        *codegenScope,
                    )

                ServiceConfig.BuilderBuild -> rust(
                    """
                    client_plugins: self.client_plugins,
                    """,
                )

                else -> emptySection
            }
        }
}

class RuntimePluginReExportCustomization(private val runtimeConfig: RuntimeConfig) {
    fun extras(rustCrate: RustCrate) {
        rustCrate.withModule(ClientRustModule.Config) {
            rustTemplate(
                """
                /// Runtime plugin configuration
                ///
                /// These are re-exported from `aws-smithy-runtime-api` for convenience.
                pub mod runtime_plugin {
                    pub use #{runtime_plugin}::{RuntimePlugins, SharedRuntimePlugin};
                }
                """,
                "runtime_plugin" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("runtime_plugin"),
            )
        }
    }
}
