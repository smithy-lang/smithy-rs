/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
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
    private val types = Types(runtimeConfig)

    private val codegenScope = arrayOf(
        *preludeScope,
        "DefaultEndpointResolver" to RuntimeType.smithyRuntime(runtimeConfig).resolve("client::orchestrator::endpoints::DefaultEndpointResolver"),
        "Endpoint" to RuntimeType.smithyHttp(runtimeConfig).resolve("endpoint::Endpoint"),
        "OldSharedEndpointResolver" to types.sharedEndpointResolver,
        "Params" to typesGenerator.paramsStruct(),
        "Resolver" to RuntimeType.smithyRuntime(runtimeConfig).resolve("client::config_override::Resolver"),
        "SharedEndpointResolver" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("client::endpoint::SharedEndpointResolver"),
        "SmithyResolver" to types.resolveEndpoint,
    )

    override fun section(section: ServiceConfig): Writable {
        return writable {
            val sharedEndpointResolver = "#{OldSharedEndpointResolver}<#{Params}>"
            val resolverTrait = "#{SmithyResolver}<#{Params}>"
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
                    // if there are no rules, we don't generate a default resolver—we need to also suppress those docs.
                    val defaultResolverDocs = if (typesGenerator.defaultResolver() != null) {
                        val endpointModule = ClientRustModule.Config.endpoint.fullyQualifiedPath()
                            .replace("crate::", "$moduleUseName::")
                        """
                        ///
                        /// When unset, the client will used a generated endpoint resolver based on the endpoint resolution
                        /// rules for `$moduleUseName`.
                        ///
                        /// ## Examples
                        /// ```no_run
                        /// use aws_smithy_http::endpoint;
                        /// use $endpointModule::{Params as EndpointParams, DefaultResolver};
                        /// /// Endpoint resolver which adds a prefix to the generated endpoint
                        /// ##[derive(Debug)]
                        /// struct PrefixResolver {
                        ///     base_resolver: DefaultResolver,
                        ///     prefix: String
                        /// }
                        /// impl endpoint::ResolveEndpoint<EndpointParams> for PrefixResolver {
                        ///   fn resolve_endpoint(&self, params: &EndpointParams) -> endpoint::Result {
                        ///        self.base_resolver
                        ///              .resolve_endpoint(params)
                        ///              .map(|ep|{
                        ///                   let url = ep.url().to_string();
                        ///                   ep.into_builder().url(format!("{}.{}", &self.prefix, url)).build()
                        ///               })
                        ///   }
                        /// }
                        /// let prefix_resolver = PrefixResolver {
                        ///     base_resolver: DefaultResolver::new(),
                        ///     prefix: "subdomain".to_string()
                        /// };
                        /// let config = $moduleUseName::Config::builder().endpoint_resolver(prefix_resolver);
                        /// ```
                        """
                    } else {
                        ""
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
                                        #{OldSharedEndpointResolver}::new(
                                            #{Endpoint}::immutable(url).expect("invalid endpoint URL")
                                        )
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
                        /// Note: setting an endpoint resolver will replace any endpoint URL that has been set.
                        ///
                        $defaultResolverDocs
                        pub fn endpoint_resolver(mut self, endpoint_resolver: impl $resolverTrait + 'static) -> Self {
                            self.set_endpoint_resolver(#{Some}(#{OldSharedEndpointResolver}::new(endpoint_resolver)));
                            self
                        }

                        /// Sets the endpoint resolver to use when making requests.
                        ///
                        /// When unset, the client will used a generated endpoint resolver based on the endpoint resolution
                        /// rules for `$moduleUseName`.
                        """,
                        *codegenScope,
                    )

                    rustTemplate(
                        """
                        pub fn set_endpoint_resolver(&mut self, endpoint_resolver: #{Option}<$sharedEndpointResolver>) -> &mut Self {
                            self.runtime_components.set_endpoint_resolver(endpoint_resolver.map(|r|#{wrap_resolver}));
                            self
                        }
                        """,
                        *codegenScope,
                        "wrap_resolver" to codegenContext.wrapResolver { rust("r") },
                    )
                }

                else -> emptySection
            }
        }
    }
}
