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

/**
 * Parses the responseBindings configuration for AWS JSON 1.0 and 1.1. It verifies the historical
 * default, independent selection of smithy-specs for each protocol, and preservation of unrelated
 * settings. Invalid values, malformed nested objects, and abbreviated protocol IDs must fail
 * with an error that identifies the offending configuration path.
 */
internal class ServerProtocolResponseSettingsTest {
    private val json10 = ShapeId.from("aws.protocols#awsJson1_0")
    private val json11 = ShapeId.from("aws.protocols#awsJson1_1")

    @Test
    fun `response bindings default to historical`() {
        for (node in listOf(null, "{}", """{"protocols":{}}""", """{"protocols":{"aws.protocols#awsJson1_0":{}}}""")) {
            val settings = settings(node)
            check(settings.responseBindings(json10) == AwsJsonResponseBindings.HISTORICAL)
            check(settings.responseBindings(json11) == AwsJsonResponseBindings.HISTORICAL)
        }
    }

    @Test
    fun `protocol flags are independent and unrelated protocol settings are preserved`() {
        for (spec in listOf("awsJson1_0", "awsJson1_1")) {
            val other = if (spec == "awsJson1_0") "awsJson1_1" else "awsJson1_0"
            val settings =
                settings(
                    """{"protocols":{"aws.protocols#$spec":{"responseBindings":"smithy-specs"},"aws.protocols#$other":{"responseBindings":"historical"},"common":{"otherSetting":1},"aws.protocols#restJson1":{"responseBindings":"smithy-specs"}}}""",
                )
            check(settings.responseBindings(ShapeId.from("aws.protocols#$spec")) == AwsJsonResponseBindings.SMITHY_SPECS)
            check(settings.responseBindings(ShapeId.from("aws.protocols#$other")) == AwsJsonResponseBindings.HISTORICAL)
            check(settings.responseBindings(ShapeId.from("aws.protocols#restJson1")) == AwsJsonResponseBindings.HISTORICAL)
            check(settings.responseBindings(ShapeId.from("smithy.protocols#rpcv2Cbor")) == AwsJsonResponseBindings.HISTORICAL)
        }
    }

    @Test
    fun `short protocol IDs are rejected even alongside full IDs`() {
        val settings = settings("""{"protocols":{"aws.protocols#awsJson1_0":{"responseBindings":"smithy-specs"}}}""")
        check(settings.responseBindings(json10) == AwsJsonResponseBindings.SMITHY_SPECS)
        expectInvalid(
            """{"protocols":{"awsJson1_0":{},"aws.protocols#awsJson1_0":{}}}""",
            "customizationConfig.protocols.awsJson1_0",
        )
    }

    @Test
    fun `invalid nested objects report their configuration path`() {
        for (value in listOf("null", "true", "1", "\"value\"", "[]")) {
            expectInvalid("""{"protocols":$value}""", "customizationConfig.protocols")
            for (protocol in listOf("aws.protocols#awsJson1_0", "aws.protocols#awsJson1_1")) {
                expectInvalid("""{"protocols":{"$protocol":$value}}""", "customizationConfig.protocols.$protocol")
            }
        }
    }

    @Test
    fun `response bindings must be a known string`() {
        for (protocol in listOf("aws.protocols#awsJson1_0", "aws.protocols#awsJson1_1")) {
            for (value in listOf("null", "1", "true", "\"legacy\"", "\"Protocol\"", "{}", "[]")) {
                expectInvalid(
                    """{"protocols":{"$protocol":{"responseBindings":$value}}}""",
                    "customizationConfig.protocols.$protocol.responseBindings",
                )
            }
        }
    }

    private fun settings(json: String?): ServerProtocolResponseSettings =
        serverTestRustSettings(
            codegenConfig = ServerCodegenConfig(schemaSerde = true),
            customizationConfig =
                json?.let {
                    ObjectNode.parse(it).expectObjectNode()
                },
        )
            .protocolResponseSettings

    private fun expectInvalid(
        json: String,
        path: String,
    ) {
        val error = assertThrows(CodegenException::class.java) { settings(json) }
        check(error.message!!.contains("`$path`"))
    }
}
