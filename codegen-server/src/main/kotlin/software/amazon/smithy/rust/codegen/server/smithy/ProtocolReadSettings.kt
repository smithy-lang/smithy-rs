/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.ShapeId
import java.util.logging.Logger

/** Request-read settings shared by all protocols served by the generated service. */
internal object ProtocolReadSettings {
    const val COMMON_PATH = "customizationConfig.protocols.common"
    const val REQUEST_BODY_READ_POLL_BUDGET_KEY = "requestBodyReadPollBudget"

    fun expectObject(
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

    /**
     * Logs a warning when `config` contains keys outside `allowed`; unknown keys are ignored
     * so configurations written for a newer codegen version keep generating.
     */
    fun warnUnknownKeys(
        config: ObjectNode,
        allowed: Set<String>,
        path: String,
    ) {
        val unknown = config.members.keys.map { it.value }.filter { it !in allowed }
        if (unknown.isNotEmpty()) {
            Logger.getLogger("ServerRustSettings").warning(
                "Unknown setting(s) in `$path`: ${unknown.joinToString(", ")}. " +
                    "Known keys are: ${allowed.joinToString(", ")}.",
            )
        }
    }

    /** Parse the protocol sections once, using the same shape-ID keys as decoupled routing. */
    fun protocolSettings(customizationConfig: ObjectNode?): Map<String, ObjectNode> {
        val protocolsPath = "customizationConfig.protocols"
        val protocols = expectObject(customizationConfig?.getMember("protocols")?.orElse(null), protocolsPath)
        return protocols?.members?.entries?.associate { (key, value) ->
            val protocolId = key.value
            val path = "$protocolsPath.$protocolId"
            if (protocolId != "common") {
                val shapeId = runCatching { ShapeId.from(protocolId) }.getOrNull()
                if (shapeId == null || shapeId.hasMember()) {
                    throw CodegenException(
                        "`$path` must use a full protocol shape ID without a member, " +
                            "such as `aws.protocols#restJson1`, or the shared `common` key. " +
                            "Protocol shorthand names are not supported.",
                    )
                }
            }
            protocolId to expectObject(value, path)!!
        }.orEmpty()
    }

    /** Fold the legacy body-size setting into `common`, preserving common-value precedence. */
    fun foldLegacyRequestBodyMaxBytes(
        customizationConfig: ObjectNode?,
        codegenConfig: ObjectNode?,
    ): ObjectNode? {
        val legacyValue = codegenConfig?.getMember("requestBodyMaxBytes")?.orElse(null) ?: return customizationConfig
        val protocols = expectObject(customizationConfig?.getMember("protocols")?.orElse(null), "customizationConfig.protocols") ?: ObjectNode.objectNode()
        val common = expectObject(protocols.getMember("common").orElse(null), COMMON_PATH) ?: ObjectNode.objectNode()
        if (common.containsMember("requestBodyMaxBytes")) {
            Logger.getLogger("ServerRustSettings").warning(
                "Both `codegen.requestBodyMaxBytes` and `$COMMON_PATH.requestBodyMaxBytes` are set; using the common value.",
            )
            return customizationConfig
        }
        return (customizationConfig ?: ObjectNode.objectNode()).withMember(
            "protocols",
            protocols.withMember("common", common.withMember("requestBodyMaxBytes", legacyValue)),
        )
    }

    /** Read a common setting from the normalized protocol map. */
    fun resolve(
        protocolSettings: Map<String, ObjectNode>,
        key: String,
    ): Pair<Node?, String> = protocolSettings["common"]?.getMember(key)?.orElse(null) to "$COMMON_PATH.$key"

    fun requestBodyMaxBytes(protocolSettings: Map<String, ObjectNode>): Long? {
        val (value, path) = resolve(protocolSettings, "requestBodyMaxBytes")
        if (value == null) return null
        val errorMessage = "`$path` must be a non-negative integer that fits in a Long"
        if (!value.isNumberNode) throw CodegenException(errorMessage)
        val maxBytes =
            try {
                value.expectNumberNode().value.toString().toBigDecimal().longValueExact()
            } catch (_: ArithmeticException) {
                throw CodegenException(errorMessage)
            }
        if (maxBytes < 0) throw CodegenException(errorMessage)
        return maxBytes
    }

    /** Ready HTTP frame budget: null inherits runtime policy; zero disables yielding. */
    fun requestBodyReadPollBudget(protocolSettings: Map<String, ObjectNode>): Int? {
        val (value, path) = resolve(protocolSettings, REQUEST_BODY_READ_POLL_BUDGET_KEY)
        if (value == null) {
            return null
        }
        val errorMessage =
            "`$path` must be an integer between 0 and ${Int.MAX_VALUE}; 0 disables forced yielding"
        if (!value.isNumberNode) {
            throw CodegenException(errorMessage)
        }
        val budget =
            try {
                value.expectNumberNode().value.toString().toBigDecimal().intValueExact()
            } catch (_: ArithmeticException) {
                throw CodegenException(errorMessage)
            } catch (_: NumberFormatException) {
                throw CodegenException(errorMessage)
            }
        if (budget < 0) {
            throw CodegenException(errorMessage)
        }
        return budget
    }
}
