/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.ShapeId

/** Independent RPC initial-message, per-event completion, and idle deadlines, in milliseconds. */
data class EventStreamTimeouts(
    val initialMessageMillis: Long,
    val messageCompletionMillis: Long,
    val idleMillis: Long,
    val perOperation: Map<ShapeId, OperationTimeouts>,
    val eventStreamInputOperationIds: Set<ShapeId>,
) {
    data class OperationTimeouts(val initialMessageMillis: Long?, val messageCompletionMillis: Long?, val idleMillis: Long?)

    fun initialMessageMillisFor(operationId: ShapeId): Long? =
        if (operationId in eventStreamInputOperationIds) {
            (perOperation[operationId]?.initialMessageMillis ?: initialMessageMillis).takeIf { it > 0 }
        } else {
            null
        }

    fun messageCompletionMillisFor(operationId: ShapeId): Long? =
        if (operationId in eventStreamInputOperationIds) {
            (perOperation[operationId]?.messageCompletionMillis ?: messageCompletionMillis).takeIf { it > 0 }
        } else {
            null
        }

    fun idleMillisFor(operationId: ShapeId): Long? =
        if (operationId in eventStreamInputOperationIds) {
            (perOperation[operationId]?.idleMillis ?: idleMillis).takeIf { it > 0 }
        } else {
            null
        }

    companion object {
        const val DEFAULT_IDLE_TIMEOUT_MILLIS = 3_600_000L
        const val CONFIG_PATH = "${ProtocolReadSettings.COMMON_PATH}.eventStreamTimeouts"

        fun fromCustomizationConfig(
            customizationConfig: ObjectNode?,
            legacyTimeouts: RequestBodyReadTimeouts,
        ): EventStreamTimeouts {
            val (node, path) =
                ProtocolReadSettings.resolve(customizationConfig, "eventStreamTimeouts", null, CONFIG_PATH)
            val config = ProtocolReadSettings.expectObject(node, path)
            if (config == null) {
                // Legacy message settings still configure event completion, never the initial read.
                return EventStreamTimeouts(
                    0,
                    legacyTimeouts.defaultEventStreamMessageMillis,
                    DEFAULT_IDLE_TIMEOUT_MILLIS,
                    legacyTimeouts.perOperationMillis
                        .filterKeys { it in legacyTimeouts.eventStreamInputOperationIds }
                        .mapValues { (_, timeout) -> OperationTimeouts(null, timeout, null) },
                    legacyTimeouts.eventStreamInputOperationIds,
                )
            }
            validateKeys(config, setOf("initialMessage", "messageCompletion", "idle", "perOperation"), path)
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
                    if (operationId !in legacyTimeouts.eventStreamInputOperationIds) {
                        throw CodegenException(
                            "`$path.perOperation` contains `$operationId`, which is not an event-stream input operation attached to this service",
                        )
                    }
                    val operationPath = "$path.perOperation.$operationId"
                    val settings = ProtocolReadSettings.expectObject(value, operationPath)!!
                    validateKeys(settings, setOf("initialMessage", "messageCompletion", "idle"), operationPath)
                    operationId to
                        OperationTimeouts(
                            duration(settings, "initialMessage", operationPath),
                            duration(settings, "messageCompletion", operationPath),
                            duration(settings, "idle", operationPath),
                        )
                }?.toMap().orEmpty()
            return EventStreamTimeouts(
                duration(config, "initialMessage", path) ?: 0,
                duration(config, "messageCompletion", path) ?: 0,
                duration(config, "idle", path) ?: DEFAULT_IDLE_TIMEOUT_MILLIS,
                overrides,
                legacyTimeouts.eventStreamInputOperationIds,
            )
        }

        private fun duration(
            config: ObjectNode,
            key: String,
            path: String,
        ): Long? =
            config.getMember(key).map { RequestBodyReadTimeouts.parseTimeoutConvertedToMillis(it, "$path.$key") }.orElse(null)

        private fun validateKeys(
            config: ObjectNode,
            allowed: Set<String>,
            path: String,
        ) {
            val unknown = config.members.keys.map { it.value }.filter { it !in allowed }
            if (unknown.isNotEmpty()) {
                throw CodegenException("Unknown setting(s) in `$path`: ${unknown.joinToString(", ")}")
            }
        }
    }
}
