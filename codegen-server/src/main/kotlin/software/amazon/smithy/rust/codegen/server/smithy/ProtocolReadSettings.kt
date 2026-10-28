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
import kotlin.time.Duration

/** Request-read settings shared by all protocols served by the generated service. */
internal object ProtocolReadSettings {
    const val COMMON_PATH = "customizationConfig.protocols.common"
    const val REQUEST_PATH = "$COMMON_PATH.request"
    const val HEADER_PATH = "$REQUEST_PATH.header"
    const val BODY_PATH = "$REQUEST_PATH.body"

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
    fun protocolSettings(
        customizationConfig: ObjectNode?,
        validateRequest: Boolean = true,
    ): Map<String, ObjectNode> {
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
            val section = expectObject(value, path)!!
            if (protocolId == "common" && validateRequest) {
                validateRequestSections(section)
            }
            protocolId to section
        }.orEmpty()
    }

    private fun validateRequestSections(common: ObjectNode) {
        warnUnknownKeys(common, setOf("request"), COMMON_PATH)
        val request = expectObject(common.getMember("request").orElse(null), REQUEST_PATH) ?: return
        warnUnknownKeys(request, setOf("header", "body"), REQUEST_PATH)
        expectObject(request.getMember("header").orElse(null), HEADER_PATH)?.let {
            warnUnknownKeys(it, setOf("readTimeout"), HEADER_PATH)
        }
        expectObject(request.getMember("body").orElse(null), BODY_PATH)?.let {
            warnUnknownKeys(it, setOf("maxBytes", "readPollBudget", "readTimeouts", "eventStreamTimeouts"), BODY_PATH)
        }
    }

    /** Fold the released legacy body-size setting into the nested request body configuration. */
    fun foldLegacyRequestBodyMaxBytes(
        customizationConfig: ObjectNode?,
        codegenConfig: ObjectNode?,
    ): ObjectNode? {
        val legacyValue = codegenConfig?.getMember("requestBodyMaxBytes")?.orElse(null) ?: return customizationConfig
        Logger.getLogger("ServerRustSettings").warning(
            "`codegen.requestBodyMaxBytes` is deprecated; use `$BODY_PATH.maxBytes` instead.",
        )
        val protocols = expectObject(customizationConfig?.getMember("protocols")?.orElse(null), "customizationConfig.protocols") ?: ObjectNode.objectNode()
        val common = expectObject(protocols.getMember("common").orElse(null), COMMON_PATH) ?: ObjectNode.objectNode()
        val request = expectObject(common.getMember("request").orElse(null), REQUEST_PATH) ?: ObjectNode.objectNode()
        val body = expectObject(request.getMember("body").orElse(null), BODY_PATH) ?: ObjectNode.objectNode()
        if (body.containsMember("maxBytes")) {
            Logger.getLogger("ServerRustSettings").warning(
                "Both `codegen.requestBodyMaxBytes` and `$BODY_PATH.maxBytes` are set; using the nested value.",
            )
            return customizationConfig
        }
        return (customizationConfig ?: ObjectNode.objectNode()).withMember(
            "protocols",
            protocols.withMember("common", common.withMember("request", request.withMember("body", body.withMember("maxBytes", legacyValue)))),
        )
    }

    /** Read a nested common setting, retaining its full path for validation errors. */
    fun resolve(
        protocolSettings: Map<String, ObjectNode>,
        vararg keys: String,
    ): Pair<Node?, String> {
        var node: Node? = protocolSettings["common"]
        var path = COMMON_PATH
        for (key in keys) {
            node = expectObject(node, path)?.getMember(key)?.orElse(null)
            path = "$path.$key"
        }
        return node to path
    }

    /** Null inherits the runtime default; zero explicitly disables the header deadline. */
    fun headerReadTimeout(protocolSettings: Map<String, ObjectNode>): Duration? {
        val (value, path) = resolve(protocolSettings, "request", "header", "readTimeout")
        return value?.let { RequestBodyReadTimeouts.parseTimeout(it, path) }
    }

    fun requestBodyMaxBytes(protocolSettings: Map<String, ObjectNode>): Long? {
        val (value, path) = resolve(protocolSettings, "request", "body", "maxBytes")
        if (value == null) {
            return null
        }
        val errorMessage =
            "`$path` must be a non-negative integer byte count or a whole-number size string with " +
                "B, KB, MB, GB, TB, KiB, MiB, GiB, or TiB units, fitting in a Long"
        val maxBytes =
            try {
                when {
                    value.isNumberNode -> value.expectNumberNode().value.toString().toBigDecimal().longValueExact()
                    value.isStringNode -> {
                        val match = BYTE_SIZE_REGEX.matchEntire(value.expectStringNode().value.trim()) ?: throw CodegenException(errorMessage)
                        val amount = match.groupValues[1].toBigInteger()
                        amount.multiply(BYTE_SIZE_UNITS.getValue(match.groupValues[2]).toBigInteger()).longValueExact()
                    }
                    else -> throw CodegenException(errorMessage)
                }
            } catch (_: ArithmeticException) {
                throw CodegenException(errorMessage)
            } catch (_: NumberFormatException) {
                throw CodegenException(errorMessage)
            }
        if (maxBytes < 0) {
            throw CodegenException(errorMessage)
        }
        return maxBytes
    }

    private val BYTE_SIZE_UNITS =
        mapOf(
            "B" to 1L,
            "KB" to 1_000L,
            "MB" to 1_000_000L,
            "GB" to 1_000_000_000L,
            "TB" to 1_000_000_000_000L,
            "KiB" to 1_024L,
            "MiB" to 1_048_576L,
            "GiB" to 1_073_741_824L,
            "TiB" to 1_099_511_627_776L,
        )
    private val BYTE_SIZE_REGEX = Regex("^([0-9]+)\\s*(B|KB|MB|GB|TB|KiB|MiB|GiB|TiB)$")

    /** Ready HTTP frame budget: null inherits runtime policy; zero disables yielding. */
    fun requestBodyReadPollBudget(protocolSettings: Map<String, ObjectNode>): Int? {
        val (value, path) = resolve(protocolSettings, "request", "body", "readPollBudget")
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
