/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import kotlin.time.Duration

/** Emits the same typed service configuration consumed by decoupled schema routing. */
class ServerServiceConfigGenerator(private val codegenContext: ServerCodegenContext) {
    private val runtime = ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType()
    private val settings = codegenContext.settings
    private val scope = arrayOf("Runtime" to runtime, *RuntimeType.preludeScope)

    fun requestBodyConfig(operation: ShapeId): Writable = operationConfig("request_body", operation)

    fun eventStreamConfig(operation: ShapeId): Writable = operationConfig("event_stream", operation)

    private fun operationConfig(
        field: String,
        operation: ShapeId,
    ): Writable =
        writable {
            rustTemplate(
                "#{ServiceConfig}().$field.for_operation(&#{ShapeId}::from_parts(${operation.toString().replace("#", "##").dq()}, ${operation.namespace.dq()}, ${operation.name.dq()}))",
                "ServiceConfig" to serviceConfig(),
                "ShapeId" to RuntimeType.smithySchema(codegenContext.runtimeConfig).resolve("ShapeId"),
            )
        }

    private fun timeout(
        value: Duration?,
        defaultName: String,
    ): Writable =
        writable {
            if (value == null) {
                rustTemplate("#{DefaultTimeout}", "DefaultTimeout" to runtime.resolve("settings::$defaultName"))
            } else {
                rustTemplate(
                    "#{Some}(#{Duration}::from_millis(${value.inWholeMilliseconds}u64))",
                    "Duration" to RuntimeType.std.resolve("time::Duration"),
                    *RuntimeType.preludeScope,
                )
            }
        }

    private fun bodyTimeout(
        value: Duration?,
        payload: Boolean,
    ): Writable =
        requiredTimeout(value, if (payload) "DEFAULT_PAYLOAD_REQUEST_BODY_READ_TIMEOUT" else "DEFAULT_NON_PAYLOAD_REQUEST_BODY_READ_TIMEOUT")

    private fun requiredTimeout(
        value: Duration?,
        defaultName: String,
    ): Writable =
        writable {
            if (value == null) {
                rustTemplate("#{Some}(#{DefaultTimeout})", "DefaultTimeout" to runtime.resolve("settings::$defaultName"), *RuntimeType.preludeScope)
            } else if (value == Duration.ZERO) {
                rustTemplate("#{None}", *RuntimeType.preludeScope)
            } else {
                rustTemplate(
                    "#{Some}(#{Duration}::from_millis(${value.inWholeMilliseconds}u64))",
                    "Duration" to RuntimeType.std.resolve("time::Duration"),
                    *RuntimeType.preludeScope,
                )
            }
        }

    /** One immutable configuration per SDK; generated serde and schema routing share its representation. */
    fun serviceConfig(): RuntimeType =
        RuntimeType.forInlineFun("service_config", RustModule.private("service_config")) {
            val operations =
                TopDownIndex.of(codegenContext.model).getContainedOperations(codegenContext.serviceShape).sortedBy {
                    it.id
                }
            val bodyTimeouts = settings.requestBodyReadTimeouts
            val bodyOverrides =
                operations.map { operation ->
                    writable {
                        val timeout =
                            if (operation.id in bodyTimeouts.streamingOperationIds) {
                                writable { rustTemplate("#{None}", *RuntimeType.preludeScope) }
                            } else {
                                bodyTimeout(bodyTimeouts.timeoutFor(operation.id), operation.id in bodyTimeouts.payloadOperationIds)
                            }
                        rustTemplate(
                            "(${operation.id.toString().replace("#", "##").dq()}.to_owned(), global.with_read_timeout(#{Timeout})),",
                            "Timeout" to timeout,
                        )
                    }
                }.join("")
            val eventTimeouts = settings.eventStreamTimeouts
            val eventOverrides =
                eventTimeouts.perOperation.entries.sortedBy { it.key }.map { (operation, override) ->
                    writable {
                        rustTemplate("(${operation.toString().replace("#", "##").dq()}.to_owned(), event_global")
                        override.initialMessageTimeout?.let {
                            rustTemplate(".with_initial_message(#{Timeout})", "Timeout" to timeout(it, "DEFAULT_EVENT_STREAM_INITIAL_MESSAGE_TIMEOUT"))
                        }
                        override.messageCompletionTimeout?.let {
                            rustTemplate(".with_message_completion(#{Timeout})", "Timeout" to timeout(it, "DEFAULT_EVENT_STREAM_MESSAGE_COMPLETION_TIMEOUT"))
                        }
                        override.idleTimeout?.let {
                            rustTemplate(".with_idle(#{Timeout})", "Timeout" to timeout(it, "DEFAULT_EVENT_STREAM_IDLE_TIMEOUT"))
                        }
                        rustTemplate("),")
                    }
                }.join("")
            val bodyOptions =
                writable {
                    settings.requestBodyMaxBytes()?.let {
                        rustTemplate(".with_max_bytes(#{NonZeroUsize}::new(${it}usize))", "NonZeroUsize" to RuntimeType.std.resolve("num::NonZeroUsize"))
                    }
                    settings.requestBodyReadPollBudget?.let {
                        rustTemplate(".with_read_poll_budget(#{NonZeroUsize}::new(${it}usize))", "NonZeroUsize" to RuntimeType.std.resolve("num::NonZeroUsize"))
                    }
                }
            rustTemplate(
                """
                pub(crate) fn service_config() -> &'static #{Runtime}::config::ServiceConfig {
                    static CONFIG: std::sync::OnceLock<#{Runtime}::config::ServiceConfig> = std::sync::OnceLock::new();
                    CONFIG.get_or_init(|| {
                        let global = #{Runtime}::config::RequestBodyCollectionConfig::default()
                            .with_read_timeout(#{GlobalBodyTimeout})#{BodyOptions};
                        let event_global = #{Runtime}::config::EventStreamTimeoutConfig::default()
                            .with_initial_message(#{InitialTimeout})
                            .with_message_completion(#{CompletionTimeout})
                            .with_idle(#{IdleTimeout});
                        #{Runtime}::config::ServiceConfig::default()
                            .with_header_read_timeout(#{HeaderTimeout})
                            .with_request_body(#{Runtime}::config::ServiceRequestBodyConfig::default()
                                .with_global(global)
                                .with_per_operation(std::collections::HashMap::from([#{BodyOverrides}])))
                            .with_event_stream(#{Runtime}::config::ServiceEventStreamConfig::default()
                                .with_global(event_global)
                                .with_per_operation(std::collections::HashMap::from([#{EventOverrides}])))
                    })
                }
                """,
                *scope,
                "HeaderTimeout" to requiredTimeout(settings.headerReadTimeout, "DEFAULT_HEADER_READ_TIMEOUT"),
                "GlobalBodyTimeout" to bodyTimeout(bodyTimeouts.defaultNonPayloadTimeout, false),
                "BodyOptions" to bodyOptions,
                "BodyOverrides" to bodyOverrides,
                "InitialTimeout" to timeout(eventTimeouts.initialMessageTimeout, "DEFAULT_EVENT_STREAM_INITIAL_MESSAGE_TIMEOUT"),
                "CompletionTimeout" to timeout(eventTimeouts.messageCompletionTimeout, "DEFAULT_EVENT_STREAM_MESSAGE_COMPLETION_TIMEOUT"),
                "IdleTimeout" to timeout(eventTimeouts.idleTimeout, "DEFAULT_EVENT_STREAM_IDLE_TIMEOUT"),
                "EventOverrides" to eventOverrides,
            )
        }
}
