/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.configReexport
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope

class IdentityCacheConfigCustomization(codegenContext: ClientCodegenContext) : ConfigCustomization() {
    private val moduleUseName = codegenContext.moduleUseName()

    private val codegenScope =
        codegenContext.runtimeConfig.let { rc ->
            val api = RuntimeType.smithyRuntimeApiClient(rc)
            arrayOf(
                *preludeScope,
                "ResolveCachedIdentity" to configReexport(api.resolve("client::identity::ResolveCachedIdentity")),
                "SharedIdentityCache" to configReexport(api.resolve("client::identity::SharedIdentityCache")),
            )
        }

    override fun section(section: ServiceConfig): Writable =
        writable {
            when (section) {
                is ServiceConfig.BuilderImpl -> {
                    val docs = """
                        /// Set the identity cache for auth.
                        ///
                        /// The identity cache defaults to a lazy caching implementation that will resolve
                        /// an identity when it is requested, and place it in the cache thereafter. Subsequent
                        /// requests will take the value from the cache while it is still valid. Once it expires,
                        /// the next request will result in refreshing the identity.
                        ///
                        /// This configuration allows you to disable or change the default caching mechanism.
                        /// To use a custom caching mechanism, implement the [`ResolveCachedIdentity`](#{ResolveCachedIdentity})
                        /// trait and pass that implementation into this function.
                        ///
                        /// ## Examples
                        ///
                        /// Disabling identity caching:
                        /// ```no_run
                        /// use $moduleUseName::config::IdentityCache;
                        ///
                        /// let config = $moduleUseName::Config::builder()
                        ///     .identity_cache(IdentityCache::no_cache())
                        ///     // ...
                        ///     .build();
                        /// let client = $moduleUseName::Client::from_conf(config);
                        /// ```
                        ///
                        /// Customizing lazy caching:
                        /// ```no_run
                        /// use $moduleUseName::config::IdentityCache;
                        /// use std::time::Duration;
                        ///
                        /// let config = $moduleUseName::Config::builder()
                        ///     .identity_cache(
                        ///         IdentityCache::lazy()
                        ///             // change the load timeout to 10 seconds
                        ///             .load_timeout(Duration::from_secs(10))
                        ///             .build()
                        ///     )
                        ///     // ...
                        ///     .build();
                        /// let client = $moduleUseName::Client::from_conf(config);
                        /// ```
                    ///"""
                    rustTemplate(
                        """
                        $docs
                        pub fn identity_cache(mut self, identity_cache: impl #{ResolveCachedIdentity} + 'static) -> Self {
                            self.set_identity_cache(identity_cache);
                            self
                        }

                        $docs
                        pub fn set_identity_cache(&mut self, identity_cache: impl #{ResolveCachedIdentity} + 'static) -> &mut Self {
                            self.runtime_components.set_identity_cache(#{Some}(identity_cache));
                            self
                        }
                        """,
                        *codegenScope,
                    )
                }

                is ServiceConfig.ConfigImpl -> {
                    rustTemplate(
                        """
                        /// Returns the configured identity cache for auth.
                        pub fn identity_cache(&self) -> #{Option}<#{SharedIdentityCache}> {
                            self.runtime_components.identity_cache()
                        }
                        """,
                        *codegenScope,
                    )
                }

                else -> {}
            }
        }
}
