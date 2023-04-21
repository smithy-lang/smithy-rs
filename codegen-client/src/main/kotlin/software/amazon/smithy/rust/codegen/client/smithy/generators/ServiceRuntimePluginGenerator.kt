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
     * Hook for adding identity resolvers.
     *
     * Should emit code that looks like the following:
     * ```
     * .identity_resolver("name", path::to::MyIdentityResolver::new())
     * ```
     */
    data class IdentityResolver(val configBagName: String) : ServiceRuntimePluginSection("IdentityResolver")

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
    data class AdditionalConfig(val configBagName: String) : ServiceRuntimePluginSection("AdditionalConfig") {
        /** Adds a value to the config bag */
        fun putConfigValue(writer: RustWriter, value: Writable) {
            writer.rust("$configBagName.put(#T);", value)
        }

        /** Generates the code to register an interceptor */
        fun registerInterceptor(runtimeConfig: RuntimeConfig, writer: RustWriter, interceptor: Writable) {
            val smithyRuntimeApi = RuntimeType.smithyRuntimeApi(runtimeConfig)
            writer.rustTemplate(
                """
                $configBagName.get::<#{Interceptors}<#{HttpRequest}, #{HttpResponse}>>()
                    .expect("interceptors set")
                    .register_client_interceptor(std::sync::Arc::new(#{interceptor}) as _);
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
        val runtimeApi = RuntimeType.smithyRuntimeApi(rc)
        val runtime = RuntimeType.smithyRuntime(rc)
        arrayOf(
            "AnonymousIdentityResolver" to runtimeApi.resolve("client::identity::AnonymousIdentityResolver"),
            "AuthOptionListResolver" to runtimeApi.resolve("client::auth::option_resolver::AuthOptionListResolver"),
            "BoxError" to runtimeApi.resolve("client::runtime_plugin::BoxError"),
            "ConfigBag" to runtimeApi.resolve("config_bag::ConfigBag"),
            "ConfigBagAccessors" to runtimeApi.resolve("client::orchestrator::ConfigBagAccessors"),
            "Connection" to runtimeApi.resolve("client::orchestrator::Connection"),
            "ConnectorSettings" to RuntimeType.smithyClient(rc).resolve("http_connector::ConnectorSettings"),
            "DefaultEndpointResolver" to runtimeApi.resolve("client::endpoints::DefaultEndpointResolver"),
            "DynConnectorAdapter" to runtime.resolve("client::connections::adapter::DynConnectorAdapter"),
            "HttpAuthSchemes" to runtimeApi.resolve("client::orchestrator::HttpAuthSchemes"),
            "IdentityResolvers" to runtimeApi.resolve("client::orchestrator::IdentityResolvers"),
            "NeverRetryStrategy" to runtimeApi.resolve("client::retries::NeverRetryStrategy"),
            "Params" to endpointTypesGenerator.paramsStruct(),
            "ResolveEndpoint" to http.resolve("endpoint::ResolveEndpoint"),
            "RuntimePlugin" to runtimeApi.resolve("client::runtime_plugin::RuntimePlugin"),
            "SharedEndpointResolver" to http.resolve("endpoint::SharedEndpointResolver"),
            "TestConnection" to runtime.resolve("client::connections::test_connection::TestConnection"),
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
                fn configure(&self, cfg: &mut #{ConfigBag}) -> Result<(), #{BoxError}> {
                    use #{ConfigBagAccessors};

                    let identity_resolvers = #{IdentityResolvers}::builder()
                        #{identity_resolver_customizations}
                        .identity_resolver("anonymous", #{AnonymousIdentityResolver}::new())
                        .build();
                    cfg.set_identity_resolvers(identity_resolvers);

                    let http_auth_schemes = #{HttpAuthSchemes}::builder()
                        #{http_auth_scheme_customizations}
                        .build();
                    cfg.set_http_auth_schemes(http_auth_schemes);

                    // Set an empty auth option resolver to be overridden by operations that need auth.
                    cfg.set_auth_option_resolver(#{AuthOptionListResolver}::new(Vec::new()));

                    let endpoint_resolver = #{DefaultEndpointResolver}::<#{Params}>::new(
                        #{SharedEndpointResolver}::from(self.handle.conf.endpoint_resolver()));
                    cfg.set_endpoint_resolver(endpoint_resolver);

                    ${"" /* TODO(EndpointResolver): Create endpoint params builder from service config */}
                    cfg.put(#{Params}::builder());

                    // TODO(RuntimePlugins): Wire up standard retry
                    cfg.set_retry_strategy(#{NeverRetryStrategy}::new());

                    // TODO(RuntimePlugins): Replace this with the correct long-term solution
                    let sleep_impl = self.handle.conf.sleep_impl();
                    let connection: Box<dyn #{Connection}> = self.handle.conf.http_connector()
                            .and_then(move |c| c.connector(&#{ConnectorSettings}::default(), sleep_impl))
                            .map(|c| Box::new(#{DynConnectorAdapter}::new(c)) as _)
                            .unwrap_or_else(|| Box::new(#{TestConnection}::new(vec![])) as _);
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
            "identity_resolver_customizations" to writable {
                writeCustomizations(customizations, ServiceRuntimePluginSection.IdentityResolver("cfg"))
            },
            "http_auth_scheme_customizations" to writable {
                writeCustomizations(customizations, ServiceRuntimePluginSection.HttpAuthScheme("cfg"))
            },
            "additional_config" to writable {
                writeCustomizations(customizations, ServiceRuntimePluginSection.AdditionalConfig("cfg"))
            },
        )
    }
}
