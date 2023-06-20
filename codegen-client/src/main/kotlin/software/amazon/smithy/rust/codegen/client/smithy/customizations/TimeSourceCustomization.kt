/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope

class TimeSourceCustomization(codegenContext: ClientCodegenContext) : ConfigCustomization() {
    private val runtimeMode = codegenContext.smithyRuntimeMode
    private val codegenScope = arrayOf(
        *preludeScope,
        "SharedTimeSource" to RuntimeType.smithyAsync(codegenContext.runtimeConfig).resolve("time::SharedTimeSource"),
    )

    override fun section(section: ServiceConfig) =
        writable {
            when (section) {
                is ServiceConfig.ConfigStruct -> {
                    if (runtimeMode.defaultToMiddleware) {
                        rustTemplate(
                            """
                            pub(crate) time_source: #{SharedTimeSource},
                            """,
                            *codegenScope,
                        )
                    }
                }

                is ServiceConfig.ConfigImpl -> {
                    rust("/// Return time source used for this service.")
                    rustBlockTemplate(
                        "pub fn time_source(&self) -> #{SharedTimeSource}",
                        *codegenScope,
                    ) {
                        if (runtimeMode.defaultToOrchestrator) {
                            rustTemplate(
                                """self.inner.load::<#{SharedTimeSource}>().expect("time source should be set").clone()""",
                                *codegenScope,
                            )
                        } else {
                            rust("self.time_source.clone()")
                        }
                    }
                }

                is ServiceConfig.BuilderStruct -> {
                    if (runtimeMode.defaultToMiddleware) {
                        rustTemplate(
                            "time_source: #{Option}<#{SharedTimeSource}>,",
                            *codegenScope,
                        )
                    }
                }

                ServiceConfig.BuilderImpl -> {
                    rustTemplate(
                        """
                        /// Sets the time source used for this service
                        pub fn time_source(
                            mut self,
                            time_source: impl #{Into}<#{SharedTimeSource}>,
                        ) -> Self {
                            self.set_time_source(#{Some}(time_source.into()));
                            self
                        }
                        """,
                        *codegenScope,
                    )

                    if (runtimeMode.defaultToOrchestrator) {
                        rustTemplate(
                            """
                            /// Sets the time source used for this service
                            pub fn set_time_source(
                                &mut self,
                                time_source: #{Option}<#{SharedTimeSource}>,
                            ) -> &mut Self {
                                self.inner.store_or_unset(time_source);
                                self
                            }
                            """,
                            *codegenScope,
                        )
                    } else {
                        rustTemplate(
                            """
                            /// Sets the time source used for this service
                            pub fn set_time_source(
                                &mut self,
                                time_source: #{Option}<#{SharedTimeSource}>,
                            ) -> &mut Self {
                                self.time_source = time_source;
                                self
                            }
                            """,
                            *codegenScope,
                        )
                    }
                }

                ServiceConfig.BuilderBuild -> {
                    if (runtimeMode.defaultToOrchestrator) {
                        rustTemplate(
                            "self.inner.store_put(self.inner.load::<#{SharedTimeSource}>().cloned().unwrap_or_default());",
                            *codegenScope,
                        )
                    } else {
                        rustTemplate(
                            "time_source: self.time_source.unwrap_or_default(),",
                            *codegenScope,
                        )
                    }
                }

                else -> emptySection
            }
        }
}

class TimeSourceOperationCustomization : OperationCustomization() {
    override fun section(section: OperationSection): Writable {
        return when (section) {
            is OperationSection.MutateRequest -> writable {
                rust(
                    """
                    ${section.request}.properties_mut().insert(${section.config}.time_source.clone());
                    """,
                )
            }

            else -> emptySection
        }
    }
}
