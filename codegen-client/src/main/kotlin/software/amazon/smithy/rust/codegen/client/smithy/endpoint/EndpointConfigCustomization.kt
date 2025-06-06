/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.serviceSpecificEndpointResolver
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope

/**
 * Customization which injects an Endpoints 2.0 Endpoint Resolver into the service config struct
 */
internal class EndpointConfigCustomization(
    private val codegenContext: ClientCodegenContext,
    private val typesGenerator: EndpointTypesGenerator,
) :
    ConfigCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val moduleUseName = codegenContext.moduleUseName()
    private val epModule = RuntimeType.smithyRuntimeApiClient(runtimeConfig).resolve("client::endpoint")
    private val epRuntimeModule = RuntimeType.smithyRuntime(runtimeConfig).resolve("client::orchestrator::endpoints")

    private val codegenScope =
        arrayOf(
            *preludeScope,
            "Params" to typesGenerator.paramsStruct(),
            "Resolver" to RuntimeType.smithyRuntime(runtimeConfig).resolve("client::config_override::Resolver"),
            "SharedEndpointResolver" to epModule.resolve("SharedEndpointResolver"),
            "StaticUriEndpointResolver" to epRuntimeModule.resolve("StaticUriEndpointResolver"),
            "ServiceSpecificResolver" to codegenContext.serviceSpecificEndpointResolver(),
            "IntoShared" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("shared::IntoShared"),
        )

    override fun section(section: ServiceConfig): Writable {
        return writable {
            when (section) {
                is ServiceConfig.ConfigImpl -> {
                    rustTemplate(
                        """
                        /// Returns the endpoint resolver.
                        pub fn endpoint_resolver(&self) -> #{SharedEndpointResolver} {
                            self.runtime_components.endpoint_resolver().expect("resolver defaulted if not set")
                        }
                        """,
                        *codegenScope,
                    )
                }

                ServiceConfig.BuilderImpl -> {
                    val endpointModule =
                        ClientRustModule.Config.endpoint.fullyQualifiedPath()
                            .replace("crate::", "$moduleUseName::")
                    // if there are no rules, we don't generate a default resolver—we need to also suppress those docs.
                    val defaultResolverDocs =
                        if (typesGenerator.defaultResolver() != null) {
                            """///
                            /// When unset, the client will used a generated endpoint resolver based on the endpoint resolution
                            /// rules for `$moduleUseName`.
                            ///"""
                        } else {
                            "/// This service does not define a default endpoint resolver."
                        }
                    if (codegenContext.settings.codegenConfig.includeEndpointUrlConfig) {
                        rustTemplate(
                            """
                            /// Set the endpoint URL to use when making requests.
                            ///
                            /// Note: setting an endpoint URL will replace any endpoint resolver that has been set.
                            ///
                            /// ## Panics
                            /// Panics if an invalid URL is given.
                            pub fn endpoint_url(mut self, endpoint_url: impl #{Into}<#{String}>) -> Self {
                                self.set_endpoint_url(#{Some}(endpoint_url.into()));
                                self
                            }

                            /// Set the endpoint URL to use when making requests.
                            ///
                            /// Note: setting an endpoint URL will replace any endpoint resolver that has been set.
                            ///
                            /// ## Panics
                            /// Panics if an invalid URL is given.
                            pub fn set_endpoint_url(&mut self, endpoint_url: #{Option}<#{String}>) -> &mut Self {
                                ##[allow(deprecated)]
                                self.set_endpoint_resolver(
                                    endpoint_url.map(|url| {
                                        #{IntoShared}::into_shared(#{StaticUriEndpointResolver}::uri(url))
                                    })
                                );
                                self
                            }
                            """,
                            *codegenScope,
                        )
                    }
                    rustTemplate(
                        """
                        /// Sets the endpoint resolver to use when making requests.
                        ///
                        $defaultResolverDocs
                        ///
                        /// Note: setting an endpoint resolver will replace any endpoint URL that has been set.
                        /// This method accepts an endpoint resolver [specific to this service](#{ServiceSpecificResolver}). If you want to
                        /// provide a shared endpoint resolver, use [`Self::set_endpoint_resolver`].
                        ///
                        /// ## Examples
                        /// Create a custom endpoint resolver that resolves a different endpoing per-stage, e.g. staging vs. production.
                        /// ```no_run
                        /// use $endpointModule::{ResolveEndpoint, EndpointFuture, Params, Endpoint};
                        /// ##[derive(Debug)]
                        /// struct StageResolver { stage: String }
                        /// impl ResolveEndpoint for StageResolver {
                        ///     fn resolve_endpoint(&self, params: &Params) -> EndpointFuture<'_> {
                        ///         let stage = &self.stage;
                        ///         EndpointFuture::ready(Ok(Endpoint::builder().url(format!("{stage}.myservice.com")).build()))
                        ///     }
                        /// }
                        /// let resolver = StageResolver { stage: std::env::var("STAGE").unwrap() };
                        /// let config = $moduleUseName::Config::builder().endpoint_resolver(resolver).build();
                        /// let client = $moduleUseName::Client::from_conf(config);
                        /// ```
                        pub fn endpoint_resolver(mut self, endpoint_resolver: impl #{ServiceSpecificResolver} + 'static) -> Self {
                            self.set_endpoint_resolver(#{Some}(endpoint_resolver.into_shared_resolver()));
                            self
                        }

                        /// Sets the endpoint resolver to use when making requests.
                        ///
                        $defaultResolverDocs
                        """,
                        *codegenScope,
                    )

                    rustTemplate(
                        """
                        pub fn set_endpoint_resolver(&mut self, endpoint_resolver: #{Option}<#{SharedEndpointResolver}>) -> &mut Self {
                            self.runtime_components.set_endpoint_resolver(endpoint_resolver);
                            self
                        }
                        """,
                        *codegenScope,
                    )
                }

                else -> emptySection
            }
        }
    }
}
