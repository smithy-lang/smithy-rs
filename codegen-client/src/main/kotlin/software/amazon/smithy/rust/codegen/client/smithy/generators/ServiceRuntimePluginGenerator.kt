/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointTypesGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
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
     * Hook for adding HTTP auth schemes.
     *
     * Should emit code that looks like the following:
     * ```
     * .auth_scheme("name", path::to::MyAuthScheme::new())
     * ```
     */
    data class HttpAuthScheme(val configBagName: String) : ServiceRuntimePluginSection("HttpAuthScheme")

    /**
     * Hook for adding retry classifiers to an operation's `RetryClassifiers` bundle.
     *
     * Should emit code that looks like the following:
     ```
     .with_classifier(AwsErrorCodeClassifier::new())
     */
    data class RetryClassifier(val configBagName: String) : ServiceRuntimePluginSection("RetryClassifier")

    /**
     * Hook for adding additional things to config inside service runtime plugins.
     */
    data class AdditionalConfig(val newLayerName: String) : ServiceRuntimePluginSection("AdditionalConfig") {
        /** Adds a value to the config bag */
        fun putConfigValue(writer: RustWriter, value: Writable) {
            writer.rust("$newLayerName.store_put(#T);", value)
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
    private val endpointTypesGenerator = EndpointTypesGenerator.fromContext(codegenContext)
    private val codegenScope = codegenContext.runtimeConfig.let { rc ->
        val http = RuntimeType.smithyHttp(rc)
        val client = RuntimeType.smithyClient(rc)
        val runtime = RuntimeType.smithyRuntime(rc)
        val runtimeApi = RuntimeType.smithyRuntimeApi(rc)
        val smithyTypes = RuntimeType.smithyTypes(rc)
        arrayOf(
            *preludeScope,
            "Arc" to RuntimeType.Arc,
            "AnonymousIdentityResolver" to runtimeApi.resolve("client::identity::AnonymousIdentityResolver"),
            "BoxError" to RuntimeType.boxError(codegenContext.runtimeConfig),
            "ConfigBag" to RuntimeType.configBag(codegenContext.runtimeConfig),
            "DynAuthOptionResolver" to runtimeApi.resolve("client::auth::DynAuthOptionResolver"),
            "Layer" to smithyTypes.resolve("config_bag::Layer"),
            "FrozenLayer" to smithyTypes.resolve("config_bag::FrozenLayer"),
            "ConfigBagAccessors" to runtimeApi.resolve("client::orchestrator::ConfigBagAccessors"),
            "Connection" to runtimeApi.resolve("client::orchestrator::Connection"),
            "ConnectorSettings" to RuntimeType.smithyClient(rc).resolve("http_connector::ConnectorSettings"),
            "DynConnectorAdapter" to runtime.resolve("client::connections::adapter::DynConnectorAdapter"),
            "HttpAuthSchemes" to runtimeApi.resolve("client::auth::HttpAuthSchemes"),
            "HttpConnector" to client.resolve("http_connector::HttpConnector"),
            "IdentityResolvers" to runtimeApi.resolve("client::identity::IdentityResolvers"),
            "InterceptorRegistrar" to runtimeApi.resolve("client::interceptors::InterceptorRegistrar"),
            "StandardRetryStrategy" to runtime.resolve("client::retries::strategy::StandardRetryStrategy"),
            "NeverRetryStrategy" to runtime.resolve("client::retries::strategy::NeverRetryStrategy"),
            "RetryClassifiers" to runtimeApi.resolve("client::retries::RetryClassifiers"),
            "Params" to endpointTypesGenerator.paramsStruct(),
            "ResolveEndpoint" to http.resolve("endpoint::ResolveEndpoint"),
            "RuntimePlugin" to runtimeApi.resolve("client::runtime_plugin::RuntimePlugin"),
            "StaticAuthOptionResolver" to runtimeApi.resolve("client::auth::option_resolver::StaticAuthOptionResolver"),
            "require_connector" to client.resolve("conns::require_connector"),
            "TimeoutConfig" to smithyTypes.resolve("timeout::TimeoutConfig"),
            "RetryConfig" to smithyTypes.resolve("retry::RetryConfig"),
        )
    }

    fun render(writer: RustWriter, customizations: List<ServiceRuntimePluginCustomization>) {
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
                    use #{ConfigBagAccessors};
                    let mut cfg = #{Layer}::new(${codegenContext.serviceShape.id.name.dq()});

                    let http_auth_schemes = #{HttpAuthSchemes}::builder()
                        #{http_auth_scheme_customizations}
                        .build();
                    cfg.set_http_auth_schemes(http_auth_schemes);

                    // Set an empty auth option resolver to be overridden by operations that need auth.
                    cfg.set_auth_option_resolver(
                        #{DynAuthOptionResolver}::new(#{StaticAuthOptionResolver}::new(#{Vec}::new()))
                    );

                    #{additional_config}

                    Some(cfg.freeze())
                }

                fn interceptors(&self, interceptors: &mut #{InterceptorRegistrar}) {
                    let _interceptors = interceptors;
                    #{additional_interceptors}
                }
            }
            """,
            *codegenScope,
            "http_auth_scheme_customizations" to writable {
                writeCustomizations(customizations, ServiceRuntimePluginSection.HttpAuthScheme("cfg"))
            },
            "additional_config" to writable {
                writeCustomizations(customizations, ServiceRuntimePluginSection.AdditionalConfig("cfg"))
            },
            "additional_interceptors" to writable {
                writeCustomizations(customizations, ServiceRuntimePluginSection.RegisterInterceptor("_interceptors"))
            },
        )
    }
}
