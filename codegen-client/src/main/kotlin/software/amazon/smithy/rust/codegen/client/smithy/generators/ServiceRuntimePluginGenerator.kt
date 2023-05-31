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
    data class AdditionalConfig(val configBagName: String, val interceptorRegistrarName: String) : ServiceRuntimePluginSection("AdditionalConfig") {
        /** Adds a value to the config bag */
        fun putConfigValue(writer: RustWriter, value: Writable) {
            writer.rust("$configBagName.put(#T);", value)
        }

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
    codegenContext: ClientCodegenContext,
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
            "BoxError" to runtimeApi.resolve("client::runtime_plugin::BoxError"),
            "ConfigBag" to smithyTypes.resolve("config_bag::ConfigBag"),
            "ConfigBagAccessors" to runtimeApi.resolve("client::orchestrator::ConfigBagAccessors"),
            "Connection" to runtimeApi.resolve("client::orchestrator::Connection"),
            "ConnectorSettings" to RuntimeType.smithyClient(rc).resolve("http_connector::ConnectorSettings"),
            "DefaultEndpointResolver" to runtime.resolve("client::orchestrator::endpoints::DefaultEndpointResolver"),
            "DynConnectorAdapter" to runtime.resolve("client::connections::adapter::DynConnectorAdapter"),
            "HttpAuthSchemes" to runtimeApi.resolve("client::auth::HttpAuthSchemes"),
            "HttpConnector" to client.resolve("http_connector::HttpConnector"),
            "IdentityResolvers" to runtimeApi.resolve("client::identity::IdentityResolvers"),
            "InterceptorRegistrar" to runtimeApi.resolve("client::interceptors::InterceptorRegistrar"),
            "NeverRetryStrategy" to runtime.resolve("client::retries::strategy::NeverRetryStrategy"),
            "RetryClassifiers" to runtimeApi.resolve("client::retries::RetryClassifiers"),
            "Params" to endpointTypesGenerator.paramsStruct(),
            "ResolveEndpoint" to http.resolve("endpoint::ResolveEndpoint"),
            "RuntimePlugin" to runtimeApi.resolve("client::runtime_plugin::RuntimePlugin"),
            "SharedEndpointResolver" to http.resolve("endpoint::SharedEndpointResolver"),
            "StaticAuthOptionResolver" to runtimeApi.resolve("client::auth::option_resolver::StaticAuthOptionResolver"),
            "default_connector" to client.resolve("conns::default_connector"),
            "require_connector" to client.resolve("conns::require_connector"),
        )
    }

    fun render(writer: RustWriter, customizations: List<ServiceRuntimePluginCustomization>) {
        writer.rustTemplate(
            """
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
                fn configure(&self, cfg: &mut #{ConfigBag}, _interceptors: &mut #{InterceptorRegistrar}) -> #{Result}<(), #{BoxError}> {
                    use #{ConfigBagAccessors};

                    // HACK: Put the handle into the config bag to work around config not being fully implemented yet
                    cfg.put(self.handle.clone());

                    let http_auth_schemes = #{HttpAuthSchemes}::builder()
                        #{http_auth_scheme_customizations}
                        .build();
                    cfg.set_http_auth_schemes(http_auth_schemes);

                    // Set an empty auth option resolver to be overridden by operations that need auth.
                    cfg.set_auth_option_resolver(#{StaticAuthOptionResolver}::new(#{Vec}::new()));

                    let endpoint_resolver = #{DefaultEndpointResolver}::<#{Params}>::new(
                        #{SharedEndpointResolver}::from(self.handle.conf.endpoint_resolver()));
                    cfg.set_endpoint_resolver(endpoint_resolver);

                    // TODO(enableNewSmithyRuntime): Use the `store_append` method of ConfigBag to insert classifiers
                    let retry_classifiers = #{RetryClassifiers}::new()
                        #{retry_classifier_customizations};
                    cfg.set_retry_classifiers(retry_classifiers);

                    // TODO(enableNewSmithyRuntime): Wire up standard retry
                    cfg.set_retry_strategy(#{NeverRetryStrategy}::new());

                    let sleep_impl = self.handle.conf.sleep_impl();
                    let timeout_config = self.handle.conf.timeout_config();
                    let connector_settings = timeout_config.map(|c| #{ConnectorSettings}::from_timeout_config(c)).unwrap_or_default();
                    let connection: #{Box}<dyn #{Connection}> = #{Box}::new(#{DynConnectorAdapter}::new(
                        // TODO(enableNewSmithyRuntime): Replace the tower-based DynConnector and remove DynConnectorAdapter when deleting the middleware implementation
                        #{require_connector}(
                            self.handle.conf.http_connector()
                                .and_then(|c| c.connector(&connector_settings, sleep_impl.clone()))
                                .or_else(|| #{default_connector}(&connector_settings, sleep_impl))
                        )?
                    )) as _;
                    cfg.set_connection(connection);

                    #{additional_config}

                    // Client-level Interceptors are registered after default Interceptors.
                    _interceptors.extend(self.handle.conf.interceptors.iter().cloned());

                    Ok(())
                }
            }
            """,
            *codegenScope,
            "http_auth_scheme_customizations" to writable {
                writeCustomizations(customizations, ServiceRuntimePluginSection.HttpAuthScheme("cfg"))
            },
            "retry_classifier_customizations" to writable {
                writeCustomizations(customizations, ServiceRuntimePluginSection.RetryClassifier("cfg"))
            },
            "additional_config" to writable {
                writeCustomizations(customizations, ServiceRuntimePluginSection.AdditionalConfig("cfg", "_interceptors"))
            },
        )
    }
}
