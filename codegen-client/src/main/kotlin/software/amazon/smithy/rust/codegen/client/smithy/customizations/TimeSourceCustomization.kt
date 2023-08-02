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
        "StaticTimeSource" to RuntimeType.smithyAsync(codegenContext.runtimeConfig).resolve("time::StaticTimeSource"),
        "UNIX_EPOCH" to RuntimeType.std.resolve("time::UNIX_EPOCH"),
        "Duration" to RuntimeType.std.resolve("time::Duration"),
    )

    override fun section(section: ServiceConfig) =
        writable {
            when (section) {
                is ServiceConfig.ConfigStruct -> {
                    if (runtimeMode.generateMiddleware) {
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
                        "pub fn time_source(&self) -> #{Option}<#{SharedTimeSource}>",
                        *codegenScope,
                    ) {
                        if (runtimeMode.generateOrchestrator) {
                            rustTemplate(
                                """self.runtime_components.time_source()""",
                                *codegenScope,
                            )
                        } else {
                            rustTemplate("#{Some}(self.time_source.clone())", *codegenScope)
                        }
                    }
                }

                is ServiceConfig.BuilderStruct -> {
                    if (runtimeMode.generateMiddleware) {
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

                    if (runtimeMode.generateOrchestrator) {
                        rustTemplate(
                            """
                            /// Sets the time source used for this service
                            pub fn set_time_source(
                                &mut self,
                                time_source: #{Option}<#{SharedTimeSource}>,
                            ) -> &mut Self {
                                self.runtime_components.set_time_source(time_source);
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
                    if (runtimeMode.generateOrchestrator) {
                        rustTemplate(
                            """
                            if self.runtime_components.time_source().is_none() {
                                self.runtime_components.set_time_source(#{Some}(#{Default}::default()));
                            }
                            """,
                            *codegenScope,
                        )
                    } else {
                        rustTemplate(
                            "time_source: self.time_source.unwrap_or_default(),",
                            *codegenScope,
                        )
                    }
                }

                is ServiceConfig.DefaultForTests -> {
                    rustTemplate(
                        """
                        ${section.configBuilderRef}
                            .set_time_source(#{Some}(#{SharedTimeSource}::new(
                                #{StaticTimeSource}::new(#{UNIX_EPOCH} + #{Duration}::from_secs(1234567890)))
                            ));
                        """,
                        *codegenScope,
                    )
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
