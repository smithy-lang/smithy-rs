/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.isNotEmpty
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.util.dq

sealed class ServiceRuntimePluginSection(name: String) : Section(name) {
    /**
     * Hook for declaring singletons that store cross-operation state.
     *
     * Examples include token buckets, ID generators, etc.
     */
    class DeclareSingletons : ServiceRuntimePluginSection("DeclareSingletons")

    /**
     * Hook for adding additional things to config inside service runtime plugins.
     */
    data class AdditionalConfig(val newLayerName: String, val serviceConfigName: String) : ServiceRuntimePluginSection("AdditionalConfig") {
        /** Adds a value to the config bag */
        fun putConfigValue(writer: RustWriter, value: Writable) {
            writer.rust("$newLayerName.store_put(#T);", value)
        }
    }

    data class RegisterRuntimeComponents(val serviceConfigName: String) : ServiceRuntimePluginSection("RegisterRuntimeComponents") {
        /** Generates the code to register an interceptor */
        fun registerInterceptor(runtimeConfig: RuntimeConfig, writer: RustWriter, interceptor: Writable) {
            writer.rustTemplate(
                """
                runtime_components.push_interceptor(#{SharedInterceptor}::new(#{interceptor}) as _);
                """,
                "interceptor" to interceptor,
                "SharedInterceptor" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("client::interceptors::SharedInterceptor"),
            )
        }

        fun registerAuthScheme(writer: RustWriter, authScheme: Writable) {
            writer.rustTemplate(
                """
                runtime_components.push_auth_scheme(#{auth_scheme});
                """,
                "auth_scheme" to authScheme,
            )
        }

        fun registerIdentityResolver(writer: RustWriter, identityResolver: Writable) {
            writer.rustTemplate(
                """
                runtime_components.push_identity_resolver(#{identity_resolver});
                """,
                "identity_resolver" to identityResolver,
            )
        }
    }
}
typealias ServiceRuntimePluginCustomization = NamedCustomization<ServiceRuntimePluginSection>

/**
 * Generates the service-level runtime plugin
 */
class ServiceRuntimePluginGenerator(
    private val codegenContext: ClientCodegenContext,
) {
    private val codegenScope = codegenContext.runtimeConfig.let { rc ->
        val runtimeApi = RuntimeType.smithyRuntimeApi(rc)
        val smithyTypes = RuntimeType.smithyTypes(rc)
        arrayOf(
            *preludeScope,
            "Arc" to RuntimeType.Arc,
            "BoxError" to RuntimeType.boxError(codegenContext.runtimeConfig),
            "Cow" to RuntimeType.Cow,
            "Layer" to smithyTypes.resolve("config_bag::Layer"),
            "FrozenLayer" to smithyTypes.resolve("config_bag::FrozenLayer"),
            "RuntimeComponentsBuilder" to RuntimeType.runtimeComponentsBuilder(rc),
            "RuntimePlugin" to RuntimeType.runtimePlugin(rc),
        )
    }

    fun render(
        writer: RustWriter,
        customizations: List<ServiceRuntimePluginCustomization>,
    ) {
        val additionalConfig = writable {
            writeCustomizations(customizations, ServiceRuntimePluginSection.AdditionalConfig("cfg", "_service_config"))
        }
        writer.rustTemplate(
            """
            ##[derive(::std::fmt::Debug)]
            pub(crate) struct ServiceRuntimePlugin {
                config: #{Option}<#{FrozenLayer}>,
                runtime_components: #{RuntimeComponentsBuilder},
            }

            impl ServiceRuntimePlugin {
                pub fn new(_service_config: crate::config::Config) -> Self {
                    let config = { #{config} };
                    let mut runtime_components = #{RuntimeComponentsBuilder}::new("ServiceRuntimePlugin");
                    #{runtime_components}
                    Self { config, runtime_components }
                }
            }

            impl #{RuntimePlugin} for ServiceRuntimePlugin {
                fn config(&self) -> #{Option}<#{FrozenLayer}> {
                    self.config.clone()
                }

                fn runtime_components(&self) -> #{Cow}<'_, #{RuntimeComponentsBuilder}> {
                    #{Cow}::Borrowed(&self.runtime_components)
                }
            }

            /// Cross-operation shared-state singletons
            #{declare_singletons}
            """,
            *codegenScope,
            "config" to writable {
                if (additionalConfig.isNotEmpty()) {
                    rustTemplate(
                        """
                        let mut cfg = #{Layer}::new(${codegenContext.serviceShape.id.name.dq()});
                        #{additional_config}
                        #{Some}(cfg.freeze())
                        """,
                        *codegenScope,
                        "additional_config" to additionalConfig,
                    )
                } else {
                    rust("None")
                }
            },
            "runtime_components" to writable {
                writeCustomizations(customizations, ServiceRuntimePluginSection.RegisterRuntimeComponents("_service_config"))
            },
            "declare_singletons" to writable {
                writeCustomizations(customizations, ServiceRuntimePluginSection.DeclareSingletons())
            },
        )
    }
}
