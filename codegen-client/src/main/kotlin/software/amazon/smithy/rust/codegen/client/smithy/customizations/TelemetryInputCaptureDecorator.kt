/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.TelemetryInputCaptureInterceptorGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope

/**
 * Wires up customer-driven telemetry input attribution:
 *
 * - a config method (`always_record_attributes`) that lets the customer name the input members to
 *   record, stored in the config bag as `RequestedTelemetryAttributes`; and
 * - a per-operation interceptor that reads that selection and captures the matching input members
 *   into `CapturedTelemetryAttributes` before the input is consumed.
 *
 * The built-in metrics implementation then carries the captured attributes. Off by default: if the
 * customer names nothing, no interceptor does any work and no attribute is captured or recorded.
 */
class TelemetryInputCaptureDecorator : ClientCodegenDecorator {
    override val name: String get() = "TelemetryInputCaptureDecorator"
    override val order: Byte get() = 0

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> = baseCustomizations + TelemetryAttributesConfigCustomization(codegenContext)

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> = baseCustomizations + TelemetryInputCaptureCustomization(codegenContext, operation)
}

private class TelemetryInputCaptureCustomization(
    private val codegenContext: ClientCodegenContext,
    private val operation: OperationShape,
) : OperationCustomization() {
    private val generator = TelemetryInputCaptureInterceptorGenerator(codegenContext)

    override fun section(section: OperationSection): Writable =
        writable {
            // No eligible members -> no interceptor for this operation.
            val interceptorName = generator.interceptorName(operation) ?: return@writable

            when (section) {
                is OperationSection.RuntimePluginSupportingTypes -> generator.render(this, operation, interceptorName)

                is OperationSection.AdditionalInterceptors ->
                    section.registerPermanentInterceptor(codegenContext.runtimeConfig, this) {
                        rust(interceptorName)
                    }

                else -> {}
            }
        }
}

private class TelemetryAttributesConfigCustomization(
    codegenContext: ClientCodegenContext,
) : ConfigCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "RequestedTelemetryAttributes" to
                RuntimeType.smithyTypes(runtimeConfig).resolve("telemetry::RequestedTelemetryAttributes"),
        )

    override fun section(section: ServiceConfig): Writable =
        writable {
            when (section) {
                is ServiceConfig.BuilderImpl ->
                    rustTemplate(
                        """
                        /// Names operation-input members whose values should be recorded as attributes
                        /// on the client's built-in metrics (e.g. `["Bucket"]`).
                        ///
                        /// Names are Smithy input member names. Only string-valued, non-sensitive members
                        /// can be recorded; naming any other member has no effect. Off by default.
                        pub fn always_record_attributes(
                            mut self,
                            names: impl #{IntoIterator}<Item = impl #{Into}<#{String}>>,
                        ) -> Self {
                            self.set_always_record_attributes(#{Some}(
                                #{RequestedTelemetryAttributes}::new(names.into_iter().map(|n| n.into())),
                            ));
                            self
                        }

                        /// See [`Self::always_record_attributes`].
                        pub fn set_always_record_attributes(
                            &mut self,
                            attributes: #{Option}<#{RequestedTelemetryAttributes}>,
                        ) -> &mut Self {
                            self.config.store_or_unset(attributes);
                            self
                        }
                        """,
                        *codegenScope,
                    )

                else -> emptySection
            }
        }
}
