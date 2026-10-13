/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import java.util.logging.Logger

/** Shared request-read settings in the same global protocol section as the decoupled server. */
internal object ProtocolReadSettings {
    const val GLOBAL_PATH = "customizationConfig.protocols.global"
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
     * The global value takes precedence, including explicit zero or an empty timeout object.
     * Legacy configuration is read only when the corresponding global key is absent.
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
        val global = expectObject(protocols?.getMember("global")?.orElse(null), GLOBAL_PATH)
        val value = global?.getMember(key)?.orElse(null)
        val legacyValue = legacyConfig?.getMember(key)?.orElse(null)
        if (value != null) {
            if (legacyValue != null) {
                Logger.getLogger("ServerRustSettings").warning(
                    "Both `$legacyPath` and `$GLOBAL_PATH.$key` are set; using `$GLOBAL_PATH.$key`.",
                )
            }
            return value to "$GLOBAL_PATH.$key"
        }
        return legacyValue to legacyPath
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
