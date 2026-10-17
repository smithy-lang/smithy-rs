/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.ShapeId

/** Server-only compatibility settings under `customizationConfig.protocols`. */
internal class ServerProtocolResponseSettings private constructor(
    private val legacyResponseBindings: Map<ShapeId, Boolean>,
) {
    fun legacyResponseBindings(protocol: ShapeId): Boolean = legacyResponseBindings[protocol] ?: false

    companion object {
        private const val PROTOCOLS_PATH = "customizationConfig.protocols"
        private const val LEGACY_RESPONSE_BINDINGS = "legacyResponseBindings"
        private val AWS_JSON_PROTOCOLS =
            listOf(
                ShapeId.from("aws.protocols#awsJson1_0"),
                ShapeId.from("aws.protocols#awsJson1_1"),
            )

        fun from(customizationConfig: ObjectNode?): ServerProtocolResponseSettings {
            val protocols =
                expectObject(customizationConfig?.getMember("protocols")?.orElse(null), PROTOCOLS_PATH)
            val values =
                AWS_JSON_PROTOCOLS.associateWith { protocol ->
                    val short = protocols?.getMember(protocol.name)?.orElse(null)
                    val full = protocols?.getMember(protocol.toString())?.orElse(null)
                    if (short != null && full != null) {
                        throw CodegenException("Use only one of `$PROTOCOLS_PATH.${protocol.name}` and `$PROTOCOLS_PATH.$protocol`")
                    }
                    val key = if (full != null) protocol.toString() else protocol.name
                    val path = "$PROTOCOLS_PATH.$key"
                    val settings = expectObject(full ?: short, path)
                    val value = settings?.getMember(LEGACY_RESPONSE_BINDINGS)?.orElse(null)
                    if (value != null && !value.isBooleanNode) {
                        throw CodegenException("`$path.$LEGACY_RESPONSE_BINDINGS` must be a JSON boolean")
                    }
                    value?.expectBooleanNode()?.value ?: false
                }
            return ServerProtocolResponseSettings(values)
        }

        private fun expectObject(
            node: Node?,
            path: String,
        ): ObjectNode? {
            if (node == null) {
                return null
            }
            if (!node.isObjectNode) {
                throw CodegenException("`$path` must be a JSON object")
            }
            return node.expectObjectNode()
        }
    }
}
