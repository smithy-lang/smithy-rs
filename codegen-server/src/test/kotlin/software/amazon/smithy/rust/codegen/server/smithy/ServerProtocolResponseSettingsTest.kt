/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Assertions.assertThrows
import org.junit.jupiter.api.Test
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestRustSettings

internal class ServerProtocolResponseSettingsTest {
    private val json10 = ShapeId.from("aws.protocols#awsJson1_0")
    private val json11 = ShapeId.from("aws.protocols#awsJson1_1")

    @Test
    fun `legacy bindings default to false`() {
        for (node in listOf(null, "{}", """{"protocols":{}}""", """{"protocols":{"awsJson1_0":{}}}""")) {
            val settings = settings(node)
            check(!settings.legacyResponseBindings(json10))
            check(!settings.legacyResponseBindings(json11))
        }
    }

    @Test
    fun `protocol flags are independent and unrelated protocol settings are preserved`() {
        for (enabled in listOf("awsJson1_0", "awsJson1_1")) {
            val disabled = if (enabled == "awsJson1_0") "awsJson1_1" else "awsJson1_0"
            val settings =
                settings(
                    """{"protocols":{"$enabled":{"legacyResponseBindings":true},"$disabled":{"legacyResponseBindings":false},"global":{"otherSetting":1},"restJson1":{"legacyResponseBindings":true}}}""",
                )
            check(settings.legacyResponseBindings(ShapeId.from("aws.protocols#$enabled")))
            check(!settings.legacyResponseBindings(ShapeId.from("aws.protocols#$disabled")))
            check(!settings.legacyResponseBindings(ShapeId.from("aws.protocols#restJson1")))
            check(!settings.legacyResponseBindings(ShapeId.from("smithy.protocols#rpcv2Cbor")))
        }
    }

    @Test
    fun `invalid nested objects report their configuration path`() {
        for (value in listOf("null", "true", "1", "\"value\"", "[]")) {
            expectInvalid("""{"protocols":$value}""", "customizationConfig.protocols")
            for (protocol in listOf("awsJson1_0", "awsJson1_1")) {
                expectInvalid("""{"protocols":{"$protocol":$value}}""", "customizationConfig.protocols.$protocol")
            }
        }
    }

    @Test
    fun `legacy response binding flags require booleans`() {
        for (protocol in listOf("awsJson1_0", "awsJson1_1")) {
            for (value in listOf("null", "1", "0", "\"true\"", "{}", "[]")) {
                expectInvalid(
                    """{"protocols":{"$protocol":{"legacyResponseBindings":$value}}}""",
                    "customizationConfig.protocols.$protocol.legacyResponseBindings",
                )
            }
        }
    }

    private fun settings(json: String?): ServerProtocolResponseSettings =
        serverTestRustSettings(customizationConfig = json?.let { ObjectNode.parse(it).expectObjectNode() })
            .protocolResponseSettings

    private fun expectInvalid(
        json: String,
        path: String,
    ) {
        val error = assertThrows(CodegenException::class.java) { settings(json) }
        check(error.message!!.contains("`$path`"))
    }
}
