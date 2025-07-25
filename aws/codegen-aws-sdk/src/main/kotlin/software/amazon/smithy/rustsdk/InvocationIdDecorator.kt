/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope

class InvocationIdDecorator : ClientCodegenDecorator {
    override val name: String get() = "InvocationIdDecorator"
    override val order: Byte get() = 0

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> =
        baseCustomizations + InvocationIdRuntimePluginCustomization(codegenContext)

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> = baseCustomizations + InvocationIdConfigCustomization(codegenContext)
}

private class InvocationIdRuntimePluginCustomization(
    private val codegenContext: ClientCodegenContext,
) : ServiceRuntimePluginCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val awsRuntime = AwsRuntimeType.awsRuntime(runtimeConfig)
    private val codegenScope =
        arrayOf(
            "InvocationIdInterceptor" to awsRuntime.resolve("invocation_id::InvocationIdInterceptor"),
        )

    override fun section(section: ServiceRuntimePluginSection): Writable =
        writable {
            if (section is ServiceRuntimePluginSection.RegisterRuntimeComponents) {
                section.registerInterceptor(this) {
                    rustTemplate("#{InvocationIdInterceptor}::new()", *codegenScope)
                }
            }
        }
}

const val GENERATOR_DOCS: String =
    "The invocation ID generator generates ID values for the `amz-sdk-invocation-id` header. " +
        "By default, this will be a random UUID. Overriding it may be useful in tests that " +
        "examine the HTTP request and need to be deterministic."

private class InvocationIdConfigCustomization(
    codegenContext: ClientCodegenContext,
) : ConfigCustomization() {
    private val awsRuntime = AwsRuntimeType.awsRuntime(codegenContext.runtimeConfig)
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "InvocationIdGenerator" to awsRuntime.resolve("invocation_id::InvocationIdGenerator"),
            "SharedInvocationIdGenerator" to awsRuntime.resolve("invocation_id::SharedInvocationIdGenerator"),
        )

    override fun section(section: ServiceConfig): Writable =
        writable {
            when (section) {
                is ServiceConfig.BuilderImpl -> {
                    docs("Overrides the default invocation ID generator.\n\n$GENERATOR_DOCS")
                    rustTemplate(
                        """
                        pub fn invocation_id_generator(mut self, gen: impl #{InvocationIdGenerator} + 'static) -> Self {
                            self.set_invocation_id_generator(#{Some}(#{SharedInvocationIdGenerator}::new(gen)));
                            self
                        }
                        """,
                        *codegenScope,
                    )

                    docs("Overrides the default invocation ID generator.\n\n$GENERATOR_DOCS")
                    rustTemplate(
                        """
                        pub fn set_invocation_id_generator(&mut self, gen: #{Option}<#{SharedInvocationIdGenerator}>) -> &mut Self {
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
                        pub fn invocation_id_generator(&self) -> #{Option}<#{SharedInvocationIdGenerator}> {
                            self.config.load::<#{SharedInvocationIdGenerator}>().cloned()
                        }
                        """,
                        *codegenScope,
                    )
                }

                else -> {}
            }
        }
}
