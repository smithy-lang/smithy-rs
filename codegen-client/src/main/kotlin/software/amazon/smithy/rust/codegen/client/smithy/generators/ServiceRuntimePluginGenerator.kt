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
     * Hook for adding additional things to config inside service runtime plugins.
     */
    data class AdditionalConfig(val newLayerName: String) : ServiceRuntimePluginSection("AdditionalConfig") {
        /** Adds a value to the config bag */
        fun putConfigValue(writer: RustWriter, value: Writable) {
            writer.rust("$newLayerName.store_put(#T);", value)
        }

        fun registerHttpAuthScheme(writer: RustWriter, runtimeConfig: RuntimeConfig, authScheme: Writable) {
            writer.rustTemplate(
                """
                #{ConfigBagAccessors}::push_http_auth_scheme(
                    &mut $newLayerName,
                    #{auth_scheme}
                );
                """,
                "ConfigBagAccessors" to RuntimeType.configBagAccessors(runtimeConfig),
                "auth_scheme" to authScheme,
            )
        }

        fun registerIdentityResolver(writer: RustWriter, runtimeConfig: RuntimeConfig, identityResolver: Writable) {
            writer.rustTemplate(
                """
                #{ConfigBagAccessors}::push_identity_resolver(
                    &mut $newLayerName,
                    #{identity_resolver}
                );
                """,
                "ConfigBagAccessors" to RuntimeType.configBagAccessors(runtimeConfig),
                "identity_resolver" to identityResolver,
            )
        }
    }

    data class RegisterInterceptor(val interceptorRegistrarName: String) : ServiceRuntimePluginSection("RegisterInterceptor") {
        /** Generates the code to register an interceptor */
        fun registerInterceptor(runtimeConfig: RuntimeConfig, writer: RustWriter, interceptor: Writable) {
            val smithyRuntimeApi = RuntimeType.smithyRuntimeApi(runtimeConfig)
            writer.rustTemplate(
                """
                $interceptorRegistrarName.register(#{SharedInterceptor}::new(#{interceptor}) as _);
                """,
                "interceptor" to interceptor,
                "SharedInterceptor" to smithyRuntimeApi.resolve("client::interceptors::SharedInterceptor"),
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
            "ConfigBag" to RuntimeType.configBag(codegenContext.runtimeConfig),
            "Layer" to smithyTypes.resolve("config_bag::Layer"),
            "FrozenLayer" to smithyTypes.resolve("config_bag::FrozenLayer"),
            "ConfigBagAccessors" to RuntimeType.configBagAccessors(rc),
            "InterceptorRegistrar" to runtimeApi.resolve("client::interceptors::InterceptorRegistrar"),
            "RuntimePlugin" to runtimeApi.resolve("client::runtime_plugin::RuntimePlugin"),
        )
    }

    fun render(
        writer: RustWriter,
        customizations: List<ServiceRuntimePluginCustomization>,
    ) {
        val additionalConfig = writable {
            writeCustomizations(customizations, ServiceRuntimePluginSection.AdditionalConfig("cfg"))
        }
        writer.rustTemplate(
            """
            // TODO(enableNewSmithyRuntimeLaunch) Remove `allow(dead_code)` as well as a field `handle` when
            //  the field is no longer used.
            ##[allow(dead_code)]
            ##[derive(Debug)]
            pub(crate) struct ServiceRuntimePlugin {
                handle: #{Arc}<crate::client::Handle>,
            }

            impl ServiceRuntimePlugin {
                pub fn new(handle: #{Arc}<crate::client::Handle>) -> Self {
                    Self { handle }
                }
            }

            impl #{RuntimePlugin} for ServiceRuntimePlugin {
                fn config(&self) -> #{Option}<#{FrozenLayer}> {
                    #{config}
                }

                fn interceptors(&self, interceptors: &mut #{InterceptorRegistrar}) {
                    let _interceptors = interceptors;
                    #{additional_interceptors}
                }
            }
            """,
            *codegenScope,
            "config" to writable {
                if (additionalConfig.isNotEmpty()) {
                    rustTemplate(
                        """
                        let mut cfg = #{Layer}::new(${codegenContext.serviceShape.id.name.dq()});

                        // TODO(enableNewSmithyRuntimeLaunch): Make it possible to set retry classifiers at the service level.
                        //     Retry classifiers can also be set at the operation level and those should be added to the
                        //     list of classifiers defined here, rather than replacing them.

                        #{additional_config}

                        Some(cfg.freeze())
                        """,
                        *codegenScope,
                        "additional_config" to additionalConfig,
                    )
                } else {
                    rust("None")
                }
            },
            "additional_interceptors" to writable {
                writeCustomizations(customizations, ServiceRuntimePluginSection.RegisterInterceptor("_interceptors"))
            },
        )
    }
}
