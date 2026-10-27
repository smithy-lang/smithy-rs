/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators.http

import software.amazon.smithy.model.knowledge.HttpBinding
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.qualifiedName
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpBindingCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpBindingGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpBindingSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpMessageType
import software.amazon.smithy.rust.codegen.core.smithy.mapRustType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpBindingDescriptor
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolFunctions
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.isStreaming
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerEventStreamUnmarshallerGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerServiceConfigGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.targetCanReachConstrainedShape

class ServerRequestBindingGenerator(
    val protocol: ServerProtocol,
    private val codegenContext: ServerCodegenContext,
    private val operationShape: OperationShape,
    private val additionalHttpBindingCustomizations: List<HttpBindingCustomization> = listOf(),
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
                ServerEventStreamMessageTimeoutHttpBindingCustomization(
                    codegenContext,
                    protocol.httpBindingResolver.isRpcProtocol(),
                ),
                ServerEventStreamPollBudgetHttpBindingCustomization(codegenContext),
            ) + additionalHttpBindingCustomizations,
        )

    fun generateDeserializeHeaderFn(binding: HttpBindingDescriptor): RuntimeType =
        httpBindingGenerator.generateDeserializeHeaderFn(binding)

    fun generateDeserializePayloadFn(
        binding: HttpBindingDescriptor,
        structuredHandler: RustWriter.(String) -> Unit,
    ): RuntimeType {
        val target = codegenContext.model.expectShape(binding.member.target)
        if (binding.member.isStreaming(codegenContext.model) && target is UnionShape) {
            return generateDeserializeEventStreamFn(binding, target)
        }
        return httpBindingGenerator.generateDeserializePayloadFn(
            binding,
            protocol.deserializePayloadErrorType(binding).toSymbol(),
            structuredHandler,
            HttpMessageType.REQUEST,
        )
    }

    private fun generateDeserializeEventStreamFn(
        binding: HttpBindingDescriptor,
        target: UnionShape,
    ): RuntimeType {
        check(binding.location == HttpBinding.Location.PAYLOAD)
        val output = codegenContext.unconstrainedShapeSymbolProvider.toSymbol(binding.member)
        val constructor = ServerEventStreamUnmarshallerGenerator(protocol, codegenContext, target).render()
        return ProtocolFunctions(codegenContext).deserializeFn(binding.member, fnNameSuffix = "payload") { fnName ->
            rustBlockTemplate(
                "pub fn $fnName(body: &mut #{SdkBody}) -> #{Result}<#{Output}, #{Error}>",
                *preludeScope,
                "SdkBody" to RuntimeType.sdkBody(codegenContext.runtimeConfig),
                "Output" to output,
                "Error" to protocol.deserializePayloadErrorType(binding),
            ) {
                rustTemplate("let unmarshaller = #{Constructor}();", "Constructor" to constructor)
                for (customization in additionalHttpBindingCustomizations) {
                    customization.section(
                        HttpBindingSection.BeforeCreatingEventStreamReceiver(operationShape, target, "unmarshaller"),
                    )(this)
                }
                rustTemplate(
                    """
                    let body = #{Replace}(body, #{SdkBody}::taken());
                    #{WrapBody}
                    let receiver = #{Receiver};
                    #{Ok}(receiver)
                    """,
                    *preludeScope,
                    "WrapBody" to
                        writable {
                            for (customization in listOf(
                                ServerEventStreamMessageTimeoutHttpBindingCustomization(codegenContext, protocol.httpBindingResolver.isRpcProtocol()),
                                ServerEventStreamPollBudgetHttpBindingCustomization(codegenContext),
                            ) + additionalHttpBindingCustomizations) {
                                customization.section(HttpBindingSection.WrapEventStreamRequestBody(operationShape, target, "body"))(this)
                            }
                        },
                    "Replace" to RuntimeType.std.resolve("mem::replace"),
                    "SdkBody" to RuntimeType.sdkBody(codegenContext.runtimeConfig),
                    "Receiver" to writable { rust("${output.rustType().qualifiedName()}::new(unmarshaller, body)") },
                )
            }
        }
    }

    fun generateDeserializePrefixHeadersFn(binding: HttpBindingDescriptor): RuntimeType =
        httpBindingGenerator.generateDeserializePrefixHeaderFn(binding)
}

/**
 * Yields after a bounded number of ready event-stream body frames, independently of
 * whether message timeouts are enabled. This runs after the timeout customization so
 * forced yields cannot interfere with the timeout body's idle-expiry EOF probe.
 */
class ServerEventStreamPollBudgetHttpBindingCustomization(val codegenContext: ServerCodegenContext) :
    HttpBindingCustomization() {
    override fun section(section: HttpBindingSection): Writable =
        when (section) {
            is HttpBindingSection.WrapEventStreamRequestBody ->
                writable {
                    val budgetExpression =
                        writable {
                            rustTemplate(
                                "#{RequestBodyConfig}.read_poll_budget.map_or(0, std::num::NonZeroUsize::get)",
                                "RequestBodyConfig" to ServerServiceConfigGenerator(codegenContext).requestBodyConfig(section.operationShape.id),
                            )
                        }
                    rustTemplate(
                        "let ${section.bodyVariableName} = #{wrap_with_poll_budget}(${section.bodyVariableName}, #{budget});",
                        "budget" to budgetExpression,
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
 * `customizationConfig.protocols.common.request.body.eventStreamTimeouts`.
 * Once the first bytes of a message arrive, the complete message frame must arrive before the
 * deadline expires, mitigating slow-drip request attacks on event stream operations.
 * For RPC protocols, this completion deadline applies only after the first frame.
 * The first frame uses only the initial-message deadline, including for stream-only inputs.
 * Disabling that deadline leaves the first RPC frame without an event-stream read deadline.
 *
 * The deadline is applied by wrapping the request body with the inlineable
 * `event_stream_message_timeout` module, with defaults supplied by the selected server runtime.
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
                    rustTemplate(
                        """
                        let event_stream_timeouts = #{EventStreamConfig};
                        let ${section.bodyVariableName} = #{wrap_with_event_stream_timeouts}(
                            ${section.bodyVariableName},
                            event_stream_timeouts.initial_message,
                            event_stream_timeouts.message_completion,
                            event_stream_timeouts.idle,
                            $readsInitialMessage,
                        );
                        """,
                        "EventStreamConfig" to ServerServiceConfigGenerator(codegenContext).eventStreamConfig(section.operationShape.id),
                        "wrap_with_event_stream_timeouts" to
                            eventStreamMessageTimeoutModule(codegenContext.runtimeConfig)
                                .resolve("wrap_with_event_stream_timeouts"),
                    )
                }
            else -> emptySection
        }

    companion object {
        /** The inlineable module applying event stream read deadlines; also referenced by
         * [software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerHttpBoundProtocolGenerator]
         * to detect initial-message timeouts and reject them as `408 Request Timeout`. */
        fun eventStreamMessageTimeoutModule(runtimeConfig: RuntimeConfig): RuntimeType =
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
                            The first RPC frame may be an `initial-request` message or an ordinary event
                            for a stream-only input. Only the initial-message deadline covers this frame.
                            If it is disabled, the first frame has no event-stream read deadline even when
                            completion or idle deadlines are enabled. Configure a nonzero
                            `request.body.eventStreamTimeouts.initialMessage` to bound incomplete first
                            frames. Completion and idle deadlines apply after the first RPC frame;
                            REST streams have no initial read and apply them from the first event.
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
}
