/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.config

import software.amazon.smithy.rust.codegen.client.smithy.SmithyRuntimeMode
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization

/**
 * Add a `token_provider` field to Service config. See below for the resulting generated code.
 */
class IdempotencyTokenProviderCustomization(private val runtimeMode: SmithyRuntimeMode) : NamedCustomization<ServiceConfig>() {

    override fun section(section: ServiceConfig): Writable {
        return when (section) {
            is ServiceConfig.ConfigStruct -> writable {
                if (runtimeMode.defaultToMiddleware) {
                    rust("pub (crate) token_provider: #T::IdempotencyTokenProvider,", RuntimeType.IdempotencyToken)
                }
            }

            ServiceConfig.ConfigImpl -> writable {
                if (runtimeMode.defaultToOrchestrator) {
                    rustTemplate(
                        """
                        /// Returns a copy of the idempotency token provider.
                        /// If a random token provider was configured,
                        /// a newly-randomized token provider will be returned.
                        pub fn token_provider(&self) -> #{IdempotencyTokenProvider} {
                            self.inner.load::<#{IdempotencyTokenProvider}>().expect("the idempotency provider should be set").clone()
                        }
                        """,
                        "IdempotencyTokenProvider" to RuntimeType.IdempotencyToken.resolve("IdempotencyTokenProvider"),
                    )
                } else {
                    rust(
                        """
                        /// Returns a copy of the idempotency token provider.
                        /// If a random token provider was configured,
                        /// a newly-randomized token provider will be returned.
                        pub fn token_provider(&self) -> #T::IdempotencyTokenProvider {
                            self.token_provider.clone()
                        }
                        """,
                        RuntimeType.IdempotencyToken,
                    )
                }
            }

            ServiceConfig.BuilderStruct -> writable {
                rust("token_provider: Option<#T::IdempotencyTokenProvider>,", RuntimeType.IdempotencyToken)
            }

            ServiceConfig.BuilderImpl -> writable {
                rustTemplate(
                    """
                    /// Sets the idempotency token provider to use for service calls that require tokens.
                    pub fn token_provider(mut self, token_provider: impl Into<#{TokenProvider}>) -> Self {
                        self.set_token_provider(Some(token_provider.into()));
                        self
                    }

                    /// Sets the idempotency token provider to use for service calls that require tokens.
                    pub fn set_token_provider(&mut self, token_provider: Option<#{TokenProvider}>) -> &mut Self {
                        self.token_provider = token_provider;
                        self
                    }
                    """,
                    "TokenProvider" to RuntimeType.IdempotencyToken.resolve("IdempotencyTokenProvider"),
                )
            }

            ServiceConfig.BuilderBuild -> writable {
                if (runtimeMode.defaultToOrchestrator) {
                    rust(
                        "layer.store_put(self.token_provider.unwrap_or_else(#T::default_provider));",
                        RuntimeType.IdempotencyToken,
                    )
                } else {
                    rust(
                        "token_provider: self.token_provider.unwrap_or_else(#T::default_provider),",
                        RuntimeType.IdempotencyToken,
                    )
                }
            }

            is ServiceConfig.DefaultForTests -> writable {
                rust("""${section.configBuilderRef}.set_token_provider(Some("00000000-0000-4000-8000-000000000000".into()));""")
            }

            else -> writable { }
        }
    }
}
