/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import java.util.logging.Logger

/** Request-read settings shared by all protocols served by the generated service. */
internal object ProtocolReadSettings {
    const val COMMON_PATH = "customizationConfig.protocols.common"
    const val EVENT_STREAM_READ_POLL_BUDGET_KEY = "eventStreamReadPollBudget"
    const val DEFAULT_EVENT_STREAM_READ_POLL_BUDGET = 32

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
     * The common value takes precedence, including explicit zero or an empty timeout object.
     * Legacy configuration is read only when the corresponding common key is absent.
     */
    fun resolve(
        customizationConfig: ObjectNode?,
        key: String,
        legacyConfig: ObjectNode?,
        legacyPath: String,
    ): Pair<Node?, String> {
        val protocols =
            expectObject(
                customizationConfig?.getMember("protocols")?.orElse(null),
                "customizationConfig.protocols",
            )
        val common = expectObject(protocols?.getMember("common")?.orElse(null), COMMON_PATH)
        val value = common?.getMember(key)?.orElse(null)
        val legacyValue = legacyConfig?.getMember(key)?.orElse(null)
        if (value != null) {
            if (legacyValue != null) {
                Logger.getLogger("ServerRustSettings").warning(
                    "Both `$legacyPath` and `$COMMON_PATH.$key` are set; using `$COMMON_PATH.$key`.",
                )
            }
            return value to "$COMMON_PATH.$key"
        }
        return legacyValue to legacyPath
    }

    /**
     * Maximum buffered bytes for a non-streaming request body from the common protocol section,
     * or `null` when the legacy `codegen.requestBodyMaxBytes` value (parsed separately, with its
     * original validation) applies.
     */
    fun requestBodyMaxBytes(
        customizationConfig: ObjectNode?,
        legacyCodegenConfig: ObjectNode?,
    ): Long? {
        val key = ServerCodegenConfig.REQUEST_BODY_MAX_BYTES_CONFIG_KEY
        val (value, path) = resolve(customizationConfig, key, legacyCodegenConfig, "codegen.$key")
        if (path != "$COMMON_PATH.$key") {
            return null
        }
        val errorMessage = "`$path` must be a non-negative integer that fits in a Long"
        if (value == null || !value.isNumberNode) {
            throw CodegenException(errorMessage)
        }
        val maxBytes =
            try {
                value.expectNumberNode().value.toString().toBigDecimal().longValueExact()
            } catch (_: ArithmeticException) {
                throw CodegenException(errorMessage)
            }
        if (maxBytes < 0) {
            throw CodegenException(errorMessage)
        }
        return maxBytes
    }

    /** Maximum consecutive ready HTTP body frames; zero disables forced yielding. */
    fun eventStreamReadPollBudget(
        customizationConfig: ObjectNode?,
        legacyCodegenConfig: ObjectNode?,
    ): Int {
        val (value, path) =
            resolve(
                customizationConfig,
                EVENT_STREAM_READ_POLL_BUDGET_KEY,
                legacyCodegenConfig,
                "codegen.$EVENT_STREAM_READ_POLL_BUDGET_KEY",
            )
        if (value == null) {
            return DEFAULT_EVENT_STREAM_READ_POLL_BUDGET
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
