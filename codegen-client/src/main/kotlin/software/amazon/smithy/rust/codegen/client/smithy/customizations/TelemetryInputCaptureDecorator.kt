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
 * - two config methods — `always_record_attributes` (capture + record on the built-in metrics) and
 *   `capture_operation_input_attributes` (capture only, for in-process reads) — that let the
 *   customer name input members, stored in the config bag as `RequestedTelemetryAttributes`; and
 * - a per-operation interceptor that reads that selection and captures the matching input members
 *   into `CapturedTelemetryAttributes` before the input is consumed.
 *
 * The built-in metrics implementation then carries only the *recorded* members. Off by default: if
 * the customer names nothing, no interceptor does any work and no attribute is captured or recorded.
 *
 * Recorded members become metric labels, so a high-cardinality member (like an object key) fragments
 * the metrics into many low-value time series and inflates cost; the `always_record_attributes` docs
 * steer customers toward bounded identifiers and toward capture-only for the rest.
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
                        /// Names operation-input members whose values are captured *and* recorded as
                        /// attributes on the client's built-in metrics (e.g. `["Bucket"]`).
                        ///
                        /// Recording implies capture, so a recorded member is also readable in-process
                        /// via `CapturedTelemetryAttributes` on the config bag. Names are Smithy input
                        /// member names; only string-valued, non-sensitive members are eligible, and
                        /// naming any other member has no effect. Off by default.
                        ///
                        /// Prefer bounded identifiers here: a recorded member becomes a metric label, so
                        /// high-cardinality values (like object keys) fragment the metrics and inflate
                        /// cost. Use [`Self::capture_operation_input_attributes`] for values you want to
                        /// read in-process without recording them.
                        pub fn always_record_attributes(
                            mut self,
                            names: impl #{IntoIterator}<Item = impl #{Into}<#{String}>>,
                        ) -> Self {
                            let mut requested = self.config.load::<#{RequestedTelemetryAttributes}>().cloned().unwrap_or_default();
                            requested.record(names.into_iter().map(|n| n.into()));
                            self.config.store_put(requested);
                            self
                        }

                        /// Names operation-input members whose values are captured into
                        /// `CapturedTelemetryAttributes` for in-process reads (e.g. from a custom
                        /// interceptor), but are **not** recorded on the built-in metrics.
                        ///
                        /// Use this for values you need during the operation lifecycle but do not want on
                        /// the metric label set (for example, high-cardinality identifiers). Names follow
                        /// the same eligibility rules as [`Self::always_record_attributes`]. Off by default.
                        pub fn capture_operation_input_attributes(
                            mut self,
                            names: impl #{IntoIterator}<Item = impl #{Into}<#{String}>>,
                        ) -> Self {
                            let mut requested = self.config.load::<#{RequestedTelemetryAttributes}>().cloned().unwrap_or_default();
                            requested.capture_only(names.into_iter().map(|n| n.into()));
                            self.config.store_put(requested);
                            self
                        }
                        """,
                        *codegenScope,
                    )

                else -> emptySection
            }
        }
}
