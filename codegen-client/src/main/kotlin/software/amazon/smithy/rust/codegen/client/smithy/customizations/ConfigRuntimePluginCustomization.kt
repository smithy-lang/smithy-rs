/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

class ConfigRuntimePluginCustomization(codegenContext: ClientCodegenContext) : ConfigCustomization() {
    private val enabled = codegenContext.smithyRuntimeMode.generateOrchestrator
    private val runtimeApi = RuntimeType.smithyRuntimeApi(codegenContext.runtimeConfig)
    private val codegenScope = arrayOf(
        "SharedRuntimePlugin" to runtimeApi.resolve("client::runtime_plugin::SharedRuntimePlugin"),
        "RuntimePlugin" to runtimeApi.resolve("client::runtime_plugin::RuntimePlugin"),
    )

    override fun section(section: ServiceConfig): Writable {
        if (!enabled) {
            return emptySection
        }
        return writable {
            when (section) {
                is ServiceConfig.ConfigStruct -> rustTemplate(
                    "pub(crate) runtime_plugins: Vec<#{SharedRuntimePlugin}>,",
                    *codegenScope,
                )

                is ServiceConfig.BuilderBuildExtras -> rustTemplate("runtime_plugins: vec![],")
                is ServiceConfig.ConfigImpl -> rustTemplate(
                    """
                    /// Clones this config and creates a copy with a new runtime plugin added
                    ///
                    /// This operation _does not_ mutate config
                    pub(crate) fn with_runtime_plugin(&self, plugin: impl #{RuntimePlugin} + 'static) -> Self {
                        let mut conf = self.clone();
                        conf.runtime_plugins.push(#{SharedRuntimePlugin}::new(plugin));
                        conf
                    }
                    """,
                    *codegenScope,
                )

                else -> {}
            }
        }
    }
}
