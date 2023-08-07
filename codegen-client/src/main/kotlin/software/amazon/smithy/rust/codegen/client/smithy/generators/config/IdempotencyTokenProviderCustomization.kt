/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.config

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization

/**
 * Add an `idempotency_token_provider` field to Service config.
 */
class IdempotencyTokenProviderCustomization(codegenContext: ClientCodegenContext) : NamedCustomization<ServiceConfig>() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val runtimeMode = codegenContext.smithyRuntimeMode
    private val codegenScope = arrayOf(
        *preludeScope,
        "default_provider" to RuntimeType.idempotencyToken(runtimeConfig).resolve("default_provider"),
        "IdempotencyTokenProvider" to RuntimeType.idempotencyToken(runtimeConfig).resolve("IdempotencyTokenProvider"),
    )

    override fun section(section: ServiceConfig): Writable {
        return when (section) {
            is ServiceConfig.ConfigStruct -> writable {
                if (runtimeMode.generateMiddleware) {
                    rustTemplate("pub (crate) idempotency_token_provider: #{IdempotencyTokenProvider},", *codegenScope)
                }
            }

            ServiceConfig.ConfigImpl -> writable {
                if (runtimeMode.generateOrchestrator) {
                    rustTemplate(
                        """
                        /// Returns a copy of the idempotency token provider.
                        /// If a random token provider was configured,
                        /// a newly-randomized token provider will be returned.
                        pub fn idempotency_token_provider(&self) -> #{IdempotencyTokenProvider} {
                            self.config.load::<#{IdempotencyTokenProvider}>().expect("the idempotency provider should be set").clone()
                        }
                        """,
                        *codegenScope,
                    )
                } else {
                    rustTemplate(
                        """
                        /// Returns a copy of the idempotency token provider.
                        /// If a random token provider was configured,
                        /// a newly-randomized token provider will be returned.
                        pub fn idempotency_token_provider(&self) -> #{IdempotencyTokenProvider} {
                            self.idempotency_token_provider.clone()
                        }
                        """,
                        *codegenScope,
                    )
                }
            }

            ServiceConfig.BuilderStruct -> writable {
                if (runtimeMode.generateMiddleware) {
                    rustTemplate("idempotency_token_provider: #{Option}<#{IdempotencyTokenProvider}>,", *codegenScope)
                }
            }

            ServiceConfig.BuilderImpl -> writable {
                rustTemplate(
                    """
                    /// Sets the idempotency token provider to use for service calls that require tokens.
                    pub fn idempotency_token_provider(mut self, idempotency_token_provider: impl #{Into}<#{IdempotencyTokenProvider}>) -> Self {
                        self.set_idempotency_token_provider(#{Some}(idempotency_token_provider.into()));
                        self
                    }
                    """,
                    *codegenScope,
                )

                if (runtimeMode.generateOrchestrator) {
                    rustTemplate(
                        """
                        /// Sets the idempotency token provider to use for service calls that require tokens.
                        pub fn set_idempotency_token_provider(&mut self, idempotency_token_provider: #{Option}<#{IdempotencyTokenProvider}>) -> &mut Self {
                            self.config.store_or_unset(idempotency_token_provider);
                            self
                        }
                        """,
                        *codegenScope,
                    )
                } else {
                    rustTemplate(
                        """
                        /// Sets the idempotency token provider to use for service calls that require tokens.
                        pub fn set_idempotency_token_provider(&mut self, idempotency_token_provider: #{Option}<#{IdempotencyTokenProvider}>) -> &mut Self {
                            self.idempotency_token_provider = idempotency_token_provider;
                            self
                        }
                        """,
                        *codegenScope,
                    )
                }
            }

            ServiceConfig.BuilderBuild -> writable {
                if (runtimeMode.generateOrchestrator) {
                    rustTemplate(
                        """
                        if !resolver.is_set::<#{IdempotencyTokenProvider}>() {
                            resolver.config_mut().store_put(#{default_provider}());
                        }
                        """,
                        *codegenScope,
                    )
                } else {
                    rustTemplate(
                        "idempotency_token_provider: self.idempotency_token_provider.unwrap_or_else(#{default_provider}),",
                        *codegenScope,
                    )
                }
            }

            is ServiceConfig.DefaultForTests -> writable {
                rust("""${section.configBuilderRef}.set_idempotency_token_provider(Some("00000000-0000-4000-8000-000000000000".into()));""")
            }

            else -> writable { }
        }
    }
}
