/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope

class TimeSourceCustomization(codegenContext: ClientCodegenContext) : ConfigCustomization() {
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "IntoShared" to RuntimeType.smithyRuntimeApi(codegenContext.runtimeConfig).resolve("shared::IntoShared"),
            "SharedTimeSource" to RuntimeType.smithyAsync(codegenContext.runtimeConfig).resolve("time::SharedTimeSource"),
            "StaticTimeSource" to RuntimeType.smithyAsync(codegenContext.runtimeConfig).resolve("time::StaticTimeSource"),
            "TimeSource" to RuntimeType.smithyAsync(codegenContext.runtimeConfig).resolve("time::TimeSource"),
            "UNIX_EPOCH" to RuntimeType.std.resolve("time::UNIX_EPOCH"),
            "Duration" to RuntimeType.std.resolve("time::Duration"),
        )

    override fun section(section: ServiceConfig) =
        writable {
            when (section) {
                is ServiceConfig.ConfigImpl -> {
                    rust("/// Return time source used for this service.")
                    rustBlockTemplate(
                        "pub fn time_source(&self) -> #{Option}<#{SharedTimeSource}>",
                        *codegenScope,
                    ) {
                        rustTemplate(
                            """self.runtime_components.time_source()""",
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
                            time_source: impl #{TimeSource} + 'static,
                        ) -> Self {
                            self.set_time_source(#{Some}(#{IntoShared}::into_shared(time_source)));
                            self
                        }
                        """,
                        *codegenScope,
                    )

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
                }

                is ServiceConfig.BuilderBuild -> {
                    rustTemplate(
                        """
                        if self.runtime_components.time_source().is_none() {
                            self.runtime_components.set_time_source(#{Some}(#{Default}::default()));
                        }
                        """,
                        *codegenScope,
                    )
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
