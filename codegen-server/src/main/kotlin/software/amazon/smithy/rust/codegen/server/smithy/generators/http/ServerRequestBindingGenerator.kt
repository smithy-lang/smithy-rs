/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators.http

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpBindingCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpBindingGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpBindingSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpMessageType
import software.amazon.smithy.rust.codegen.core.smithy.mapRustType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpBindingDescriptor
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.targetCanReachConstrainedShape
import kotlin.time.Duration

class ServerRequestBindingGenerator(
    val protocol: ServerProtocol,
    codegenContext: ServerCodegenContext,
    operationShape: OperationShape,
    additionalHttpBindingCustomizations: List<HttpBindingCustomization> = listOf(),
) {
    private val httpBindingGenerator =
        HttpBindingGenerator(
            protocol,
            codegenContext,
            // Note how we parse the HTTP-bound values into _unconstrained_ types; they will be constrained when
            // building the builder.
            codegenContext.unconstrainedShapeSymbolProvider,
            operationShape,
            listOf(
                ServerRequestAfterDeserializingIntoAHashMapOfHttpPrefixHeadersWrapInUnconstrainedMapHttpBindingCustomization(
                    codegenContext,
                ),
                ServerEventStreamPollBudgetHttpBindingCustomization(codegenContext),
                ServerEventStreamMessageTimeoutHttpBindingCustomization(
                    codegenContext,
                    protocol.httpBindingResolver.isRpcProtocol(),
                ),
            ) + additionalHttpBindingCustomizations,
        )

    fun generateDeserializeHeaderFn(binding: HttpBindingDescriptor): RuntimeType =
        httpBindingGenerator.generateDeserializeHeaderFn(binding)

    fun generateDeserializePayloadFn(
        binding: HttpBindingDescriptor,
        structuredHandler: RustWriter.(String) -> Unit,
    ): RuntimeType =
        httpBindingGenerator.generateDeserializePayloadFn(
            binding,
            protocol.deserializePayloadErrorType(binding).toSymbol(),
            structuredHandler,
            HttpMessageType.REQUEST,
        )

    fun generateDeserializePrefixHeadersFn(binding: HttpBindingDescriptor): RuntimeType =
        httpBindingGenerator.generateDeserializePrefixHeaderFn(binding)
}

/**
 * Yields after a bounded number of ready event-stream body frames, independently of
 * whether message timeouts are enabled. This runs before the timeout customization so
 * expiration is checked outside the polling-budget wrapper.
 */
class ServerEventStreamPollBudgetHttpBindingCustomization(val codegenContext: ServerCodegenContext) :
    HttpBindingCustomization() {
    override fun section(section: HttpBindingSection): Writable =
        when (section) {
            is HttpBindingSection.WrapEventStreamRequestBody ->
                writable {
                    val budget = codegenContext.settings.eventStreamReadPollBudget
                    if (budget > 0) {
                        rustTemplate(
                            "let ${section.bodyVariableName} = #{wrap_with_poll_budget}(${section.bodyVariableName}, ${budget}usize);",
                            "wrap_with_poll_budget" to
                                RuntimeType.forInlineDependency(
                                    InlineDependency.forRustFile(
                                        RustModule.private("event_stream_poll_budget"),
                                        "/inlineable/src/event_stream_poll_budget.rs",
                                        CargoDependency.smithyTypes(codegenContext.runtimeConfig).withFeature("http-body-1-x"),
                                        CargoDependency.Bytes,
                                        CargoDependency.HttpBody1x,
                                        CargoDependency.HttpBodyUtil01x.toDevDependency(),
                                        CargoDependency.Tokio.toDevDependency(),
                                        ServerCargoDependency.PinProjectLite,
                                    ),
                                ).resolve("wrap_with_poll_budget"),
                        )
                    }
                }
            else -> emptySection
        }
}

/**
 * A customization to, just after we've deserialized HTTP request headers bound to a map shape via `@httpPrefixHeaders`,
 * wrap the `std::collections::HashMap` in an unconstrained type wrapper newtype.
 */
class ServerRequestAfterDeserializingIntoAHashMapOfHttpPrefixHeadersWrapInUnconstrainedMapHttpBindingCustomization(val codegenContext: ServerCodegenContext) :
    HttpBindingCustomization() {
    override fun section(section: HttpBindingSection): Writable =
        when (section) {
            is HttpBindingSection.BeforeRenderingHeaderValue,
            is HttpBindingSection.BeforeIteratingOverMapShapeBoundWithHttpPrefixHeaders,
            -> emptySection
            is HttpBindingSection.AfterDeserializingIntoAHashMapOfHttpPrefixHeaders ->
                writable {
                    if (section.memberShape.targetCanReachConstrainedShape(codegenContext.model, codegenContext.unconstrainedShapeSymbolProvider)) {
                        rust(
                            "let out = out.map(#T);",
                            codegenContext.unconstrainedShapeSymbolProvider.toSymbol(section.memberShape).mapRustType {
                                it.stripOuter<RustType.Option>()
                            },
                        )
                    }
                }
            else -> emptySection
        }
}

/**
 * A customization to apply a per-message completion deadline to the event stream request body
 * handed to the operation handler, configured via
 * `customizationConfig.protocols.common.eventStreamTimeouts`.
 * Once the first bytes of a message arrive, the complete message frame must arrive before the
 * deadline expires, mitigating slow-drip request attacks on event stream operations.
 *
 * The deadline is applied by wrapping the request body with the inlineable
 * `event_stream_message_timeout` module, keeping the runtime change local to the generated crate.
 */
class ServerEventStreamMessageTimeoutHttpBindingCustomization(
    val codegenContext: ServerCodegenContext,
    private val readsInitialMessage: Boolean,
) :
    HttpBindingCustomization() {
    override fun section(section: HttpBindingSection): Writable =
        when (section) {
            is HttpBindingSection.WrapEventStreamRequestBody ->
                writable {
                    val timeouts = codegenContext.settings.eventStreamTimeouts
                    val messageTimeout = timeouts.messageCompletionTimeoutFor(section.operationShape.id)
                    val idleTimeout = timeouts.idleTimeoutFor(section.operationShape.id)
                    val initialTimeout =
                        if (readsInitialMessage) timeouts.initialMessageTimeoutFor(section.operationShape.id) else null
                    if (messageTimeout != null || initialTimeout != null || idleTimeout != null) {
                        fun duration(value: Duration?): Writable =
                            writable {
                                if (value == null) {
                                    rustTemplate("#{None}", *RuntimeType.preludeScope)
                                } else {
                                    rustTemplate(
                                        "#{Some}(#{Duration}::from_millis(${value.inWholeMilliseconds}u64))",
                                        *RuntimeType.preludeScope,
                                        "Duration" to RuntimeType.std.resolve("time::Duration"),
                                    )
                                }
                            }
                        rustTemplate(
                            """
                            let ${section.bodyVariableName} = #{wrap_with_event_stream_timeouts}(
                                ${section.bodyVariableName},
                                #{initialTimeout},
                                #{messageTimeout},
                                #{idleTimeout},
                                $readsInitialMessage,
                            );
                            """,
                            "initialTimeout" to duration(initialTimeout),
                            "messageTimeout" to duration(messageTimeout),
                            "idleTimeout" to duration(idleTimeout),
                            "wrap_with_event_stream_timeouts" to
                                eventStreamMessageTimeoutModule(codegenContext.runtimeConfig)
                                    .resolve("wrap_with_event_stream_timeouts"),
                        )
                    }
                }
            else -> emptySection
        }

    private fun eventStreamMessageTimeoutModule(runtimeConfig: RuntimeConfig): RuntimeType =
        RuntimeType.forInlineDependency(
            InlineDependency.forRustFile(
                // Public so handlers can downcast to `MessageTimeoutError` and decide whether to
                // stop processing or keep waiting (by calling `recv()` again).
                RustModule.public(
                    "event_stream_message_timeout",
                    documentationOverride =
                        """
                        Per-message read deadlines for event stream request bodies.

                        When an event stream message starts arriving but doesn't complete within the
                        configured timeout, the event stream receiver's `recv()` call returns an error
                        whose source chain contains [`MessageTimeoutError`](event_stream_message_timeout::MessageTimeoutError).
                        The error is resumable: a handler may call `recv()` again to grant the message
                        one more timeout window, or propagate the error to end request processing.
                        The timeout does not itself close the transport connection.

                        RPC initial reads have a separate deadline starting before their first bytes
                        arrive. Initial-message timeouts are request rejections before the handler runs.
                        A separate idle deadline bounds waiting between complete messages and the
                        next message's first bytes. `MessageTimeoutError::is_idle()` identifies it.
                        All event stream deadlines are disabled by default.

                        Deadlines are checked before reading more body frames. Time spent between
                        polls counts toward the deadline, even when the remaining bytes are buffered.
                        """.trimIndent(),
                ),
                "/inlineable/src/event_stream_message_timeout.rs",
                CargoDependency.smithyTypes(runtimeConfig).withFeature("http-body-1-x"),
                CargoDependency.Bytes,
                CargoDependency.HttpBody1x,
                CargoDependency.HttpBodyUtil01x.toDevDependency(),
                CargoDependency.Tokio.toDevDependency(),
                ServerCargoDependency.PinProjectLite,
                ServerCargoDependency.TokioTime,
                CargoDependency.Tracing,
            ),
        )
}
