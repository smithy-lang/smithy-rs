/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.smithyRuntime

const val GENERATOR_DOCS: String =
    "The invocation ID generator generates ID values for client operation invocation. " +
        "By default, this will be a random UUID per client. Overriding it may be useful in tests that " +
        "examine the HTTP request and need to be deterministic."

class InvocationIdConfigCustomization(codegenContext: ClientCodegenContext) : ConfigCustomization() {
    private val smithyRuntime = smithyRuntime(codegenContext.runtimeConfig)
    private val codegenScope =
        arrayOf(
            *RuntimeType.preludeScope,
            "GenerateInvocationId" to smithyRuntime.resolve("client::invocation_id::GenerateInvocationId"),
            "SharedInvocationIdGenerator" to smithyRuntime.resolve("client::invocation_id::SharedInvocationIdGenerator"),
        )

    override fun section(section: ServiceConfig): Writable =
        writable {
            when (section) {
                is ServiceConfig.BuilderImpl -> {
                    docs("Overrides the default invocation ID generator.\n\n$GENERATOR_DOCS")
                    rustTemplate(
                        """
                        pub fn invocation_id_generator_v2(mut self, gen: impl #{GenerateInvocationId} + 'static) -> Self {
                            self.set_invocation_id_generator_v2(#{Some}(#{SharedInvocationIdGenerator}::new(gen)));
                            self
                        }
                        """,
                        *codegenScope,
                    )

                    docs("Overrides the default invocation ID generator.\n\n$GENERATOR_DOCS")
                    rustTemplate(
                        """
                        pub fn set_invocation_id_generator_v2(&mut self, gen: #{Option}<#{SharedInvocationIdGenerator}>) -> &mut Self {
                            self.config.store_or_unset(gen);
                            self
                        }
                        """,
                        *codegenScope,
                    )
                }

                is ServiceConfig.ConfigImpl -> {
                    docs("Returns the invocation ID generator if one was given in config.\n\n$GENERATOR_DOCS")
                    rustTemplate(
                        """
                        pub fn invocation_id_generator_v2(&self) -> #{Option}<#{SharedInvocationIdGenerator}> {
                            self.config.load::<#{SharedInvocationIdGenerator}>().cloned()
                        }
                        """,
                        *codegenScope,
                    )
                }

                is ServiceConfig.BuilderFromConfigBag -> {
                    writable {
                        rustTemplate(
                            "${section.builder}.set_invocation_id_generator_v2(${section.configBag}.load::<#{SharedInvocationIdGenerator}>().cloned());",
                            *codegenScope,
                        )
                    }
                }

                else -> {}
            }
        }
}
