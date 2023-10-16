/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations

sealed class ServiceRuntimePluginSection(name: String) : Section(name) {
    /**
     * Hook for declaring singletons that store cross-operation state.
     *
     * Examples include token buckets, ID generators, etc.
     */
    class DeclareSingletons : ServiceRuntimePluginSection("DeclareSingletons")

    data class RegisterRuntimeComponents(val serviceConfigName: String) : ServiceRuntimePluginSection("RegisterRuntimeComponents") {
        /** Generates the code to register an interceptor */
        fun registerInterceptor(writer: RustWriter, interceptor: Writable) {
            writer.rust("runtime_components.push_interceptor(#T);", interceptor)
        }

        fun registerAuthScheme(writer: RustWriter, authScheme: Writable) {
            writer.rust("runtime_components.push_auth_scheme(#T);", authScheme)
        }

        fun registerEndpointResolver(writer: RustWriter, resolver: Writable) {
            writer.rust("runtime_components.set_endpoint_resolver(Some(#T));", resolver)
        }

        fun registerIdentityResolver(writer: RustWriter, identityResolver: Writable) {
            writer.rust("runtime_components.push_identity_resolver(#T);", identityResolver)
        }

        fun registerRetryClassifier(writer: RustWriter, classifier: Writable) {
            writer.rust("runtime_components.push_retry_classifier(#T);", classifier)
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
            "FrozenLayer" to smithyTypes.resolve("config_bag::FrozenLayer"),
            "IntoShared" to runtimeApi.resolve("shared::IntoShared"),
            "Layer" to smithyTypes.resolve("config_bag::Layer"),
            "RuntimeComponentsBuilder" to RuntimeType.runtimeComponentsBuilder(rc),
            "RuntimePlugin" to RuntimeType.runtimePlugin(rc),
        )
    }

    fun render(
        writer: RustWriter,
        customizations: List<ServiceRuntimePluginCustomization>,
    ) {
        writer.rustTemplate(
            """
            ##[derive(::std::fmt::Debug)]
            pub(crate) struct ServiceRuntimePlugin {
                runtime_components: #{RuntimeComponentsBuilder},
            }

            impl ServiceRuntimePlugin {
                pub fn new(_service_config: crate::config::Config) -> Self {
                    let mut runtime_components = #{RuntimeComponentsBuilder}::new("ServiceRuntimePlugin");
                    #{runtime_components}
                    Self { runtime_components }
                }
            }

            impl #{RuntimePlugin} for ServiceRuntimePlugin {
                fn config(&self) -> #{Option}<#{FrozenLayer}> {
                    None
                }

                fn runtime_components(&self, _: &#{RuntimeComponentsBuilder}) -> #{Cow}<'_, #{RuntimeComponentsBuilder}> {
                    #{Cow}::Borrowed(&self.runtime_components)
                }
            }

            /// Cross-operation shared-state singletons
            #{declare_singletons}
            """,
            *codegenScope,
            "runtime_components" to writable {
                writeCustomizations(customizations, ServiceRuntimePluginSection.RegisterRuntimeComponents("_service_config"))
            },
            "declare_singletons" to writable {
                writeCustomizations(customizations, ServiceRuntimePluginSection.DeclareSingletons())
            },
        )
    }
}
