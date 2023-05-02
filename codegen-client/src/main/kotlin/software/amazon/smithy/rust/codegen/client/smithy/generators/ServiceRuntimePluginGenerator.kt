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
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations

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
     * Hook for adding additional things to config inside service runtime plugins.
     */
    data class AdditionalConfig(val configBagName: String, val interceptorName: String) : ServiceRuntimePluginSection("AdditionalConfig") {
        /** Adds a value to the config bag */
        fun putConfigValue(writer: RustWriter, value: Writable) {
            writer.rust("$configBagName.put(#T);", value)
        }

        /** Generates the code to register an interceptor */
        fun registerInterceptor(runtimeConfig: RuntimeConfig, writer: RustWriter, interceptor: Writable) {
            val smithyRuntimeApi = RuntimeType.smithyRuntimeApi(runtimeConfig)
            writer.rustTemplate(
                """
                $interceptorName.register_client_interceptor(std::sync::Arc::new(#{interceptor}) as _);
                """,
                "HttpRequest" to smithyRuntimeApi.resolve("client::orchestrator::HttpRequest"),
                "HttpResponse" to smithyRuntimeApi.resolve("client::orchestrator::HttpResponse"),
                "Interceptors" to smithyRuntimeApi.resolve("client::interceptors::Interceptors"),
                "interceptor" to interceptor,
            )
        }
    }
}
typealias ServiceRuntimePluginCustomization = NamedCustomization<ServiceRuntimePluginSection>

/**
 * Generates the service-level runtime plugin
 */
class ServiceRuntimePluginGenerator(
    codegenContext: ClientCodegenContext,
) {
    private val endpointTypesGenerator = EndpointTypesGenerator.fromContext(codegenContext)
    private val codegenScope = codegenContext.runtimeConfig.let { rc ->
        val http = RuntimeType.smithyHttp(rc)
        val runtime = RuntimeType.smithyRuntime(rc)
        val runtimeApi = RuntimeType.smithyRuntimeApi(rc)
        arrayOf(
            "AnonymousIdentityResolver" to runtimeApi.resolve("client::identity::AnonymousIdentityResolver"),
            "BoxError" to runtimeApi.resolve("client::runtime_plugin::BoxError"),
            "ConfigBag" to runtimeApi.resolve("config_bag::ConfigBag"),
            "ConfigBagAccessors" to runtimeApi.resolve("client::orchestrator::ConfigBagAccessors"),
            "Connection" to runtimeApi.resolve("client::orchestrator::Connection"),
            "ConnectorSettings" to RuntimeType.smithyClient(rc).resolve("http_connector::ConnectorSettings"),
            "DefaultEndpointResolver" to runtime.resolve("client::orchestrator::endpoints::DefaultEndpointResolver"),
            "DynConnectorAdapter" to runtime.resolve("client::connections::adapter::DynConnectorAdapter"),
            "HttpAuthSchemes" to runtimeApi.resolve("client::auth::HttpAuthSchemes"),
            "IdentityResolvers" to runtimeApi.resolve("client::identity::IdentityResolvers"),
            "NeverRetryStrategy" to runtime.resolve("client::retries::strategy::NeverRetryStrategy"),
            "Params" to endpointTypesGenerator.paramsStruct(),
            "ResolveEndpoint" to http.resolve("endpoint::ResolveEndpoint"),
            "RuntimePlugin" to runtimeApi.resolve("client::runtime_plugin::RuntimePlugin"),
            "Interceptors" to runtimeApi.resolve("client::interceptors::Interceptors"),
            "SharedEndpointResolver" to http.resolve("endpoint::SharedEndpointResolver"),
            "StaticAuthOptionResolver" to runtimeApi.resolve("client::auth::option_resolver::StaticAuthOptionResolver"),
            "TraceProbe" to runtimeApi.resolve("client::orchestrator::TraceProbe"),
        )
    }

    fun render(writer: RustWriter, customizations: List<ServiceRuntimePluginCustomization>) {
        writer.rustTemplate(
            """
            pub(crate) struct ServiceRuntimePlugin {
                handle: std::sync::Arc<crate::client::Handle>,
            }

            impl ServiceRuntimePlugin {
                pub fn new(handle: std::sync::Arc<crate::client::Handle>) -> Self {
                    Self { handle }
                }
            }

            impl #{RuntimePlugin} for ServiceRuntimePlugin {
                fn configure(&self, cfg: &mut #{ConfigBag}, _interceptors: &mut #{Interceptors}) -> Result<(), #{BoxError}> {
                    use #{ConfigBagAccessors};

                    // HACK: Put the handle into the config bag to work around config not being fully implemented yet
                    cfg.put(self.handle.clone());

                    let http_auth_schemes = #{HttpAuthSchemes}::builder()
                        #{http_auth_scheme_customizations}
                        .build();
                    cfg.set_http_auth_schemes(http_auth_schemes);

                    // Set an empty auth option resolver to be overridden by operations that need auth.
                    cfg.set_auth_option_resolver(#{StaticAuthOptionResolver}::new(Vec::new()));

                    let endpoint_resolver = #{DefaultEndpointResolver}::<#{Params}>::new(
                        #{SharedEndpointResolver}::from(self.handle.conf.endpoint_resolver()));
                    cfg.set_endpoint_resolver(endpoint_resolver);

                    // TODO(RuntimePlugins): Wire up standard retry
                    cfg.set_retry_strategy(#{NeverRetryStrategy}::new());

                    // TODO(RuntimePlugins): Replace this with the correct long-term solution
                    let sleep_impl = self.handle.conf.sleep_impl();
                    let connection: Box<dyn #{Connection}> = self.handle.conf.http_connector()
                            .and_then(move |c| c.connector(&#{ConnectorSettings}::default(), sleep_impl))
                            .map(|c| Box::new(#{DynConnectorAdapter}::new(c)) as _)
                            .expect("connection set");
                    cfg.set_connection(connection);

                    // TODO(RuntimePlugins): Add the TraceProbe to the config bag
                    cfg.set_trace_probe({
                        ##[derive(Debug)]
                        struct StubTraceProbe;
                        impl #{TraceProbe} for StubTraceProbe {
                            fn dispatch_events(&self) {
                                // no-op
                            }
                        }
                        StubTraceProbe
                    });

                    #{additional_config}
                    Ok(())
                }
            }
            """,
            *codegenScope,
            "http_auth_scheme_customizations" to writable {
                writeCustomizations(customizations, ServiceRuntimePluginSection.HttpAuthScheme("cfg"))
            },
            "additional_config" to writable {
                writeCustomizations(customizations, ServiceRuntimePluginSection.AdditionalConfig("cfg", "_interceptors"))
            },
        )
    }
}
