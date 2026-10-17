/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.ShapeId

/**
 * How an AWS JSON server answers operations whose output or errors carry REST bindings
 * (`@http(code)`, `@httpError`, `@httpHeader`).
 */
internal enum class AwsJsonResponseBindings(val setting: String) {
    /** What smithy-rs AWS JSON servers have always sent: modeled statuses and header members. The default. */
    HISTORICAL("historical"),

    /** The AWS JSON specification: status 200 (errors use the `@error` class) and every member in the document. */
    PROTOCOL("protocol"),
}

/**
 * Server-only settings under `customizationConfig.protocols.<awsJson1_0|awsJson1_1>.responseBindings`.
 *
 * Only the schema runtime (`codegen.schemaSerde: true`) reads the value; legacy generated servers
 * always behave as [AwsJsonResponseBindings.HISTORICAL]. The setting is validated here so a typo fails codegen.
 */
internal class ServerProtocolResponseSettings private constructor(
    private val responseBindings: Map<ShapeId, AwsJsonResponseBindings>,
) {
    fun responseBindings(protocol: ShapeId): AwsJsonResponseBindings =
        responseBindings[protocol] ?: AwsJsonResponseBindings.HISTORICAL

    companion object {
        private const val PROTOCOLS_PATH = "customizationConfig.protocols"
        private const val RESPONSE_BINDINGS = "responseBindings"
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
                    val value = settings?.getMember(RESPONSE_BINDINGS)?.orElse(null)
                    if (value == null) {
                        AwsJsonResponseBindings.HISTORICAL
                    } else {
                        AwsJsonResponseBindings.values().firstOrNull { value.isStringNode && it.setting == value.expectStringNode().value }
                            ?: throw CodegenException(
                                "`$path.$RESPONSE_BINDINGS` must be one of " +
                                    AwsJsonResponseBindings.values().joinToString { "\"${it.setting}\"" },
                            )
                    }
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
