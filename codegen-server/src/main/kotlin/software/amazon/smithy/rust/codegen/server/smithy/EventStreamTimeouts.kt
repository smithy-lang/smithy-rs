/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.ShapeId
import kotlin.time.Duration

/** Independent RPC initial-message, per-event completion, and idle deadlines. */
data class EventStreamTimeouts(
    val initialMessageTimeout: Duration,
    val messageCompletionTimeout: Duration,
    val idleTimeout: Duration,
    val perOperation: Map<ShapeId, OperationTimeouts>,
    val eventStreamInputOperationIds: Set<ShapeId>,
) {
    data class OperationTimeouts(val initialMessageTimeout: Duration?, val messageCompletionTimeout: Duration?, val idleTimeout: Duration?)

    fun initialMessageTimeoutFor(operationId: ShapeId): Duration? =
        if (operationId in eventStreamInputOperationIds) {
            (perOperation[operationId]?.initialMessageTimeout ?: initialMessageTimeout).takeIf { it > Duration.ZERO }
        } else {
            null
        }

    fun messageCompletionTimeoutFor(operationId: ShapeId): Duration? =
        if (operationId in eventStreamInputOperationIds) {
            (perOperation[operationId]?.messageCompletionTimeout ?: messageCompletionTimeout).takeIf { it > Duration.ZERO }
        } else {
            null
        }

    fun idleTimeoutFor(operationId: ShapeId): Duration? =
        if (operationId in eventStreamInputOperationIds) {
            (perOperation[operationId]?.idleTimeout ?: idleTimeout).takeIf { it > Duration.ZERO }
        } else {
            null
        }

    companion object {
        val DEFAULT_IDLE_TIMEOUT = Duration.ZERO
        const val CONFIG_PATH = "${ProtocolReadSettings.COMMON_PATH}.eventStreamTimeouts"

        fun fromCustomizationConfig(
            customizationConfig: ObjectNode?,
            requestBodyReadTimeouts: RequestBodyReadTimeouts,
        ): EventStreamTimeouts {
            val eventStreamInputOperationIds = requestBodyReadTimeouts.eventStreamInputOperationIds
            val (node, path) = ProtocolReadSettings.resolve(customizationConfig, "eventStreamTimeouts")
            val config = ProtocolReadSettings.expectObject(node, path)
            if (config == null) {
                return EventStreamTimeouts(
                    Duration.ZERO,
                    Duration.ZERO,
                    DEFAULT_IDLE_TIMEOUT,
                    emptyMap(),
                    eventStreamInputOperationIds,
                )
            }
            ProtocolReadSettings.warnUnknownKeys(config, setOf("initialMessage", "messageCompletion", "idle", "perOperation"), path)
            val operationNode =
                ProtocolReadSettings.expectObject(config.getMember("perOperation").orElse(null), "$path.perOperation")
            val overrides =
                operationNode?.members?.map { (key, value) ->
                    val operationId =
                        try {
                            ShapeId.from(key.value)
                        } catch (error: RuntimeException) {
                            throw CodegenException("`$path.perOperation` contains invalid operation ID `${key.value}`", error)
                        }
                    if (operationId !in eventStreamInputOperationIds) {
                        throw CodegenException(
                            "`$path.perOperation` contains `$operationId`, which is not an event-stream input operation attached to this service",
                        )
                    }
                    val operationPath = "$path.perOperation.$operationId"
                    val settings = ProtocolReadSettings.expectObject(value, operationPath)!!
                    ProtocolReadSettings.warnUnknownKeys(settings, setOf("initialMessage", "messageCompletion", "idle"), operationPath)
                    operationId to
                        OperationTimeouts(
                            duration(settings, "initialMessage", operationPath),
                            duration(settings, "messageCompletion", operationPath),
                            duration(settings, "idle", operationPath),
                        )
                }?.toMap().orEmpty()
            return EventStreamTimeouts(
                duration(config, "initialMessage", path) ?: Duration.ZERO,
                duration(config, "messageCompletion", path) ?: Duration.ZERO,
                duration(config, "idle", path) ?: DEFAULT_IDLE_TIMEOUT,
                overrides,
                eventStreamInputOperationIds,
            )
        }

        private fun duration(
            config: ObjectNode,
            key: String,
            path: String,
        ): Duration? = config.getMember(key).map { RequestBodyReadTimeouts.parseTimeout(it, "$path.$key") }.orElse(null)
    }
}
