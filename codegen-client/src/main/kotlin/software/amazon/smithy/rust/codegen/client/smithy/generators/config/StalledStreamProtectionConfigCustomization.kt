/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.config

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.util.isStreaming

class StalledStreamProtectionOperationCustomization(
    private val codegenContext: ClientCodegenContext,
) : OperationCustomization() {
    private val rc = codegenContext.runtimeConfig

    override fun section(section: OperationSection): Writable = writable {
        when (section) {
            is OperationSection.FeatureGatedComponents -> {
                val model = codegenContext.model
                val stalledStreamProtectionModule = RuntimeType.smithyRuntime(rc).resolve("client::stalled_stream_protection")
                // Only bother mounting this interceptor when an operation's input and/or output
                // has a blob shape member.
                val inputHasBlobMember = section.operationShape.inputShape
                    ?.let { inputShape -> model.expectShape(inputShape).members() }
                    ?.any { it.isStreaming(model) } ?: false;
                val outputHasBlobMember = section.operationShape.outputShape
                    ?.let { outputShape -> model.expectShape(outputShape).members()
                    .any { it.isStreaming(model) } } ?: false;

                val kind = when (inputHasBlobMember to outputHasBlobMember) {
                    true to true -> "RequestAndResponseBody"
                    true to false -> "RequestBody"
                    false to true -> "ResponseBody"
                    else -> return@writable
                }

                if (inputHasBlobMember || outputHasBlobMember) {
                    section.registerInterceptor("stalled-stream-protection", this) {
                        rustTemplate(
                            """
                            #{StalledStreamProtectionInterceptor}::new(#{Kind}::$kind)
                            """,
                            *preludeScope,
                            "StalledStreamProtectionInterceptor" to stalledStreamProtectionModule.resolve("StalledStreamProtectionInterceptor"),
                            "Kind" to stalledStreamProtectionModule.resolve("StalledStreamProtectionInterceptorKind")
                        )
                    }
                }
            }

            else -> { }
        }
    }
}

/**
 * Add a `stalled_stream_protection_config` field to Service config.
 */
class StalledStreamProtectionConfigCustomization(codegenContext: ClientCodegenContext) : NamedCustomization<ServiceConfig>() {
    private val rc = codegenContext.runtimeConfig
    private val codegenScope = arrayOf(
        *preludeScope,
        "StalledStreamProtectionConfig" to RuntimeType.smithyTypes(rc).resolve("stalled_stream_protection::StalledStreamProtectionConfig"),
    )

    override fun section(section: ServiceConfig): Writable {
        return when (section) {
            ServiceConfig.ConfigImpl -> writable {
                Attribute.featureGate("stalled-stream-protection").render(this)
                rustTemplate(
                    """
                    /// Return a reference to the stalled stream protection configuration contained in this config, if any.
                    pub fn stalled_stream_protection_config(&self) -> #{Option}<&#{StalledStreamProtectionConfig}> {
                        self.config.load::<#{StalledStreamProtectionConfig}>()
                    }
                    """,
                    *codegenScope,
                )
            }
            ServiceConfig.BuilderImpl -> writable {
                Attribute.featureGate("stalled-stream-protection").render(this)
                rustTemplate(
                    """
                    /// Set the [`StalledStreamProtectionConfig`](#{StalledStreamProtectionConfig})
                    /// to configure protection for stalled streams.
                    pub fn stalled_stream_protection_config(
                        mut self,
                        stalled_stream_protection_config: impl #{Into}<#{StalledStreamProtectionConfig}>
                    ) -> Self {
                        self.set_stalled_stream_protection_config(#{Some}(stalled_stream_protection_config.into()));
                        self
                    }
                    """,
                    *codegenScope,
                )

                Attribute.featureGate("stalled-stream-protection").render(this)
                rustTemplate(
                    """
                    /// Set the [`StalledStreamProtectionConfig`](#{StalledStreamProtectionConfig})
                    /// to configure protection for stalled streams.
                    pub fn set_stalled_stream_protection_config(
                        &mut self,
                        stalled_stream_protection_config: #{Option}<#{StalledStreamProtectionConfig}>
                    ) -> &mut Self {
                        self.config.store_or_unset(stalled_stream_protection_config);
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

class StalledStreamProtectionConfigReExportCustomization(codegenContext: ClientCodegenContext) {
    private val rc = codegenContext.runtimeConfig
    private val feature = Feature("stalled-stream-protection", false, listOf("aws-smithy-types/stalled-stream-protection", "aws-smithy-runtime/stalled-stream-protection"))

    fun extras(rustCrate: RustCrate) {
        rustCrate.mergeFeature(feature)

        rustCrate.withModule(ClientRustModule.config) {
            Attribute.featureGate(feature.name).render(this)
            rustTemplate(
                "pub use #{StalledStreamProtectionConfig};",
                "StalledStreamProtectionConfig" to RuntimeType.smithyTypes(rc).resolve("stalled_stream_protection::StalledStreamProtectionConfig"),
            )
        }
    }
}
