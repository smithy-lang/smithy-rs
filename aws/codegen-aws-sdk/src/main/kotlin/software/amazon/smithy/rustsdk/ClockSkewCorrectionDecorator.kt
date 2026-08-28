/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization

class ClockSkewCorrectionDecorator : ClientCodegenDecorator {
    override val name: String = "ClockSkewCorrection"
    override val order: Byte = 0

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> =
        baseCustomizations + ClockSkewCorrectionOperationCustomization(codegenContext, operation)

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> = baseCustomizations + ClockSkewCorrectionConfigCustomization(codegenContext)

    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> =
        listOf(
            adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
                rust(
                    """
                    ${section.serviceConfigBuilder} = ${section.serviceConfigBuilder}
                        .disable_clock_skew_correction(${section.sdkConfig}.disable_clock_skew_correction());
                    """,
                )
            },
        )
}

private class ClockSkewCorrectionOperationCustomization(
    codegenContext: ClientCodegenContext,
    private val operation: OperationShape,
) : OperationCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val symbolProvider = codegenContext.symbolProvider

    override fun section(section: OperationSection) =
        when (section) {
            is OperationSection.RetryClassifiers ->
                writable {
                    section.registerRetryClassifier(this) {
                        rustTemplate(
                            "#{ServiceClockSkewClassifier}::<#{OperationError}>::new()",
                            "ServiceClockSkewClassifier" to
                                AwsRuntimeType.awsRuntime(runtimeConfig)
                                    .resolve("service_clock_skew::ServiceClockSkewClassifier"),
                            "OperationError" to symbolProvider.symbolForOperationError(operation),
                        )
                    }
                }

            else -> emptySection
        }
}

private class ClockSkewCorrectionConfigCustomization(codegenContext: ClientCodegenContext) : ConfigCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            "DisableClockSkewCorrection" to
                AwsRuntimeType.awsRuntime(runtimeConfig).resolve("service_clock_skew::DisableClockSkewCorrection"),
            *preludeScope,
        )

    override fun section(section: ServiceConfig) =
        writable {
            when (section) {
                ServiceConfig.ConfigImpl -> {
                    rustTemplate(
                        """
                        /// Returns the `disable clock skew correction` setting, if it was provided.
                        pub fn disable_clock_skew_correction(&self) -> #{Option}<bool> {
                            self.config.load::<#{DisableClockSkewCorrection}>().map(|it| it.is_disabled())
                        }
                        """,
                        *codegenScope,
                    )
                }

                ServiceConfig.BuilderImpl -> {
                    rustTemplate(
                        """
                        /// Sets whether clock skew correction is disabled when making requests.
                        pub fn disable_clock_skew_correction(mut self, disable_clock_skew_correction: impl #{Into}<#{Option}<bool>>) -> Self {
                            self.set_disable_clock_skew_correction(disable_clock_skew_correction.into());
                            self
                        }

                        /// Sets whether clock skew correction is disabled when making requests.
                        pub fn set_disable_clock_skew_correction(&mut self, disable_clock_skew_correction: #{Option}<bool>) -> &mut Self {
                            self.config.store_or_unset::<#{DisableClockSkewCorrection}>(disable_clock_skew_correction.map(Into::into));
                            self
                        }
                        """,
                        *codegenScope,
                    )
                }

                is ServiceConfig.BuilderFromConfigBag -> {
                    rustTemplate(
                        """
                        ${section.builder}.set_disable_clock_skew_correction(
                            ${section.configBag}.load::<#{DisableClockSkewCorrection}>().map(|it| it.is_disabled()));
                        """,
                        *codegenScope,
                    )
                }

                else -> emptySection
            }
        }
}
