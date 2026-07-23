/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.model.shapes.EnumShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.SensitiveTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.inputShape

/**
 * Generates a per-operation interceptor that captures selected input members into the config bag
 * for telemetry.
 *
 * The customer opts in by naming input members on the service config (see the telemetry config
 * customization). At `read_before_execution` — before the orchestrator consumes the input during
 * serialization — this interceptor downcasts the typed input, and for each member the customer
 * requested, writes its value into [`CapturedTelemetryAttributes`] on the config bag. Downstream
 * interceptors and the built-in metrics can then read it via `cfg.load`.
 *
 * Selection is customer-driven: there is no model marker and no curated allow-list. Only
 * string-valued, non-`@sensitive` members are eligible — resource identifiers such as `Bucket`,
 * `Key`, `TableName`, and `QueueUrl` are all strings. `@sensitive` members are excluded so a
 * sensitive value can never be recorded even if named.
 */
class TelemetryInputCaptureInterceptorGenerator(
    private val codegenContext: ClientCodegenContext,
) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val runtimeConfig = codegenContext.runtimeConfig

    private val codegenScope =
        arrayOf(
            *preludeScope,
            "BoxError" to RuntimeType.boxError(runtimeConfig),
            "ConfigBag" to RuntimeType.configBag(runtimeConfig),
            "Intercept" to RuntimeType.intercept(runtimeConfig),
            "dyn_dispatch_hint" to RuntimeType.dynDispatchHint(runtimeConfig),
            "BeforeSerializationInterceptorContextRef" to
                RuntimeType.beforeSerializationInterceptorContextRef(runtimeConfig),
            "Input" to
                RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                    .resolve("client::interceptors::context::Input"),
            "Output" to
                RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                    .resolve("client::interceptors::context::Output"),
            "Error" to
                RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                    .resolve("client::interceptors::context::Error"),
            "CapturedTelemetryAttributes" to
                RuntimeType.smithyTypes(runtimeConfig).resolve("telemetry::CapturedTelemetryAttributes"),
            "RequestedTelemetryAttributes" to
                RuntimeType.smithyTypes(runtimeConfig).resolve("telemetry::RequestedTelemetryAttributes"),
        )

    /**
     * Members eligible for capture: plain-string-valued and not `@sensitive`.
     *
     * `@sensitive` is checked on the target shape (where Smithy allows the trait); the member-level
     * check is cheap defence in case a model transform synthesizes it. A sensitive value must never
     * be recorded.
     *
     * Enums are excluded even though `EnumShape`/`@enum`-trait strings are `StringShape` subtypes:
     * they render as Rust enum types (not `String`), so they have no `Deref` for the `as_deref()`
     * capture arm, and they are not the free-form resource identifiers this feature targets.
     */
    private fun eligibleMembers(operationShape: OperationShape) =
        operationShape.inputShape(model).members().filter { member ->
            val target = model.expectShape(member.target)
            !member.hasTrait(SensitiveTrait::class.java) &&
                !target.hasTrait(SensitiveTrait::class.java) &&
                target is StringShape &&
                target !is EnumShape &&
                !target.hasTrait(EnumTrait::class.java)
        }

    /**
     * Returns `null` when the operation has no eligible members — nothing to generate, so no
     * interceptor is emitted for it.
     */
    fun interceptorName(operationShape: OperationShape): String? {
        if (eligibleMembers(operationShape).isEmpty()) return null
        val operationName = symbolProvider.toSymbol(operationShape).name
        return "${operationName}TelemetryInputCaptureInterceptor"
    }

    fun render(
        writer: RustWriter,
        operationShape: OperationShape,
        interceptorName: String,
    ) {
        val operationInput = symbolProvider.toSymbol(operationShape.inputShape(model))

        writer.rustTemplate(
            """
            ##[derive(Debug)]
            struct $interceptorName;

            ##[#{dyn_dispatch_hint}]
            impl #{Intercept} for $interceptorName {
                fn name(&self) -> &'static str {
                    ${interceptorName.dq()}
                }

                fn read_before_execution(
                    &self,
                    context: &#{BeforeSerializationInterceptorContextRef}<'_, #{Input}, #{Output}, #{Error}>,
                    cfg: &mut #{ConfigBag},
                ) -> #{Result}<(), #{BoxError}> {
                    // Nothing to do unless the customer opted in by naming members to record.
                    let #{Some}(requested) = cfg.load::<#{RequestedTelemetryAttributes}>().filter(|r| !r.is_empty()) else {
                        return #{Ok}(());
                    };

                    let #{Some}(input) = context.input().downcast_ref::<${operationInput.name}>() else {
                        // A mismatched input is not this interceptor's concern; skip quietly.
                        return #{Ok}(());
                    };

                    let mut captured = #{CapturedTelemetryAttributes}::default();
                    #{capture_arms}

                    cfg.interceptor_state().store_put(captured);
                    #{Ok}(())
                }
            }
            """,
            *codegenScope,
            "capture_arms" to captureArms(operationShape),
        )
    }

    private fun captureArms(operationShape: OperationShape): Writable =
        writable {
            eligibleMembers(operationShape).forEach { member ->
                val memberName = symbolProvider.toMemberName(member)
                // The Smithy member name is what the customer names in their list.
                val smithyName = member.memberName
                rustTemplate(
                    """
                    if requested.contains(${smithyName.dq()}) {
                        if let #{Some}(value) = input.$memberName.as_deref() {
                            captured.insert(${smithyName.dq()}, value);
                        }
                    }
                    """,
                    *preludeScope,
                )
            }
        }
}
