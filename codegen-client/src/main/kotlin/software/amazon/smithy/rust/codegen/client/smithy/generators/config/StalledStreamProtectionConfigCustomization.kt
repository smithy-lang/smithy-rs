/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.config

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.configReexport
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.traits.IncompatibleWithStalledStreamProtectionTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization

class StalledStreamProtectionDecorator : ClientCodegenDecorator {
    override val name: String = "StalledStreamProtection"
    override val order: Byte = 0

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations + StalledStreamProtectionConfigCustomization(codegenContext)
    }

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        return baseCustomizations + StalledStreamProtectionOperationCustomization(codegenContext, operation)
    }
}

/**
 * Add a `stalled_stream_protection` field to Service config.
 */
class StalledStreamProtectionConfigCustomization(codegenContext: ClientCodegenContext) : NamedCustomization<ServiceConfig>() {
    private val rc = codegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "StalledStreamProtectionConfig" to configReexport(RuntimeType.smithyRuntimeApi(rc).resolve("client::stalled_stream_protection::StalledStreamProtectionConfig")),
        )

    override fun section(section: ServiceConfig): Writable {
        return when (section) {
            ServiceConfig.ConfigImpl ->
                writable {
                    rustTemplate(
                        """
                        /// Return a reference to the stalled stream protection configuration contained in this config, if any.
                        pub fn stalled_stream_protection(&self) -> #{Option}<&#{StalledStreamProtectionConfig}> {
                            self.config.load::<#{StalledStreamProtectionConfig}>()
                        }
                        """,
                        *codegenScope,
                    )
                }
            ServiceConfig.BuilderImpl ->
                writable {
                    rustTemplate(
                        """
                        /// Set the [`StalledStreamProtectionConfig`](#{StalledStreamProtectionConfig})
                        /// to configure protection for stalled streams.
                        pub fn stalled_stream_protection(
                            mut self,
                            stalled_stream_protection_config: #{StalledStreamProtectionConfig}
                        ) -> Self {
                            self.set_stalled_stream_protection(#{Some}(stalled_stream_protection_config));
                            self
                        }
                        """,
                        *codegenScope,
                    )

                    rustTemplate(
                        """
                        /// Set the [`StalledStreamProtectionConfig`](#{StalledStreamProtectionConfig})
                        /// to configure protection for stalled streams.
                        pub fn set_stalled_stream_protection(
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

            is ServiceConfig.BuilderFromConfigBag ->
                writable {
                    rustTemplate(
                        "${section.builder}.set_stalled_stream_protection(${section.configBag}.load::<#{StalledStreamProtectionConfig}>().cloned());",
                        *codegenScope,
                    )
                }

            else -> emptySection
        }
    }
}

class StalledStreamProtectionOperationCustomization(
    codegenContext: ClientCodegenContext,
    private val operationShape: OperationShape,
) : OperationCustomization() {
    private val rc = codegenContext.runtimeConfig

    override fun section(section: OperationSection): Writable =
        writable {
            // Don't add the stalled stream protection interceptor if the operation is incompatible with it.
            // The absence of the stalled stream protection interceptor does not remove
            // `StalledStreamProtectionConfig` from `ConfigBag`, nor does it disable the config. However, it will prevent
            // `UploadThroughput` from being added to `ConfigBag`, which will eventually cause
            // `MaybeUploadThroughputCheckFuture` to use the regular `HttpConnectorFuture` instead of `UploadThroughputCheckFuture`.
            // `MinimumThroughputDownloadBody` will not be used, as it is only added by the stalled stream protection interceptor.
            if (operationShape.hasTrait(IncompatibleWithStalledStreamProtectionTrait.ID)) {
                return@writable
            }

            when (section) {
                is OperationSection.AdditionalInterceptors -> {
                    val stalledStreamProtectionModule = RuntimeType.smithyRuntime(rc).resolve("client::stalled_stream_protection")
                    section.registerInterceptor(rc, this) {
                        rustTemplate(
                            """
                            #{StalledStreamProtectionInterceptor}::default()
                            """,
                            *preludeScope,
                            "StalledStreamProtectionInterceptor" to stalledStreamProtectionModule.resolve("StalledStreamProtectionInterceptor"),
                        )
                    }
                }
                else -> { }
            }
        }
}
