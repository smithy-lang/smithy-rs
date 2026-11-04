/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import io.kotest.assertions.throwables.shouldThrow
import io.kotest.matchers.shouldBe
import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rust.codegen.core.smithy.HttpVersion
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import java.util.logging.Handler
import java.util.logging.Level
import java.util.logging.LogRecord
import java.util.logging.Logger

/**
 * Checks how server configuration is parsed before generating Rust. It covers schemaSerde and
 * HTTP-version defaults, requestBodyMaxBytes defaults and validation, precedence between common
 * protocol settings and legacy flags, and migration of capitalized CBOR routes. It also checks
 * full protocol-ID validation and preservation of customization sections. These are configuration
 * parsing tests; generated-server tests separately verify their effects on HTTP requests.
 */
internal class ServerRustSettingsTest {
    private val model =
        """
        ${'$'}version: "2"
        namespace test

        service TestService {
            version: "1"
        }
        """.asSmithyModel()

    private fun settings(
        codegenSettings: String,
        customizationConfig: String? = null,
    ): ServerRustSettings =
        ServerRustSettings.from(
            model,
            Node.parse(
                """
                {
                    "service": "test#TestService",
                    "module": "test-service",
                    "moduleVersion": "1.0.0",
                    "moduleAuthors": ["test@example.com"],
                    "codegen": {
                        $codegenSettings
                    }${customizationConfig?.let { """, "customizationConfig": $it""" } ?: ""}
                }
                """,
            ).expectObjectNode(),
        )

    private fun schemaSettings(
        codegenSettings: String,
        customizationConfig: String? = null,
    ): ServerRustSettings =
        settings(
            listOf("\"schemaSerde\": true", codegenSettings).filter {
                it.isNotEmpty()
            }.joinToString(","),
            customizationConfig,
        )

    @Test
    fun `schemaSerde is off by default`() {
        settings("").codegenConfig.schemaSerde shouldBe false
    }

    @Test
    fun `schemaSerde can be turned on from the codegen config`() {
        settings(""""${ServerCodegenConfig.SCHEMA_SERDE_CONFIG_KEY}": true""").codegenConfig.schemaSerde shouldBe true
    }

    @Test
    fun `schemaSerde enables HTTP 1x when the HTTP setting is omitted`() {
        val settings = settings(""""schemaSerde": true""")
        settings.codegenConfig.http1x shouldBe true
        settings.runtimeConfig.httpVersion shouldBe HttpVersion.Http1x
    }

    @Test
    fun `schemaSerde accepts explicitly enabled HTTP 1x`() {
        val settings = settings(""""schemaSerde": true, "http-1x": true""")
        settings.codegenConfig.http1x shouldBe true
        settings.runtimeConfig.httpVersion shouldBe HttpVersion.Http1x
    }

    @Test
    fun `schemaSerde rejects explicitly disabled HTTP 1x`() {
        val error =
            shouldThrow<IllegalArgumentException> {
                settings(""""schemaSerde": true, "http-1x": false""")
            }
        error.message shouldBe
            "`codegen.schemaSerde` requires HTTP 1.x; `codegen.http-1x` cannot be false when `codegen.schemaSerde` is true. " +
            "Set `codegen.http-1x` to true or omit it."
    }

    @Test
    fun `HTTP defaults and explicit choices are preserved without schemaSerde`() {
        for (schemaSerde in listOf("", """"schemaSerde": false""")) {
            val defaults = settings(schemaSerde)
            defaults.codegenConfig.http1x shouldBe false
            defaults.runtimeConfig.httpVersion shouldBe HttpVersion.Http0x
            for (http1x in listOf(false, true)) {
                val settings =
                    settings(listOf(schemaSerde, """"http-1x": $http1x""").filter { it.isNotEmpty() }.joinToString(", "))
                settings.codegenConfig.http1x shouldBe http1x
                settings.runtimeConfig.httpVersion shouldBe
                    if (http1x) {
                        HttpVersion.Http1x
                    } else {
                        HttpVersion.Http0x
                    }
            }
        }
    }

    @Test
    fun `protocol settings are empty by default`() {
        settings("").protocolSettings() shouldBe emptyMap()
        settings("").rpcV2CborCapitalizeRoutes() shouldBe false
    }

    private fun bodySettings(value: String): String = """{"protocols":{"common":{"request":{"body":$value}}}}"""

    @Test
    fun `nested request body limits are read in both serde modes`() {
        for (schemaSerde in listOf(false, true)) {
            val codegen = "\"schemaSerde\":$schemaSerde"
            settings(codegen).requestBodyMaxBytes() shouldBe null
            for ((value, expected) in listOf("0" to 0L, "2048" to 2048L, "\"2 KiB\"" to 2048L, "\"2 KB\"" to 2000L)) {
                settings(codegen, bodySettings("""{"maxBytes":$value}""")).requestBodyMaxBytes() shouldBe expected
            }
        }
    }

    @Test
    fun `protocol-specific header timeouts fail eagerly in both serde modes`() {
        for (schemaSerde in listOf(false, true)) {
            for (value in listOf("\"1s\"", "0", "null")) {
                val error =
                    shouldThrow<CodegenException> {
                        settings(
                            "\"schemaSerde\":$schemaSerde",
                            """{"protocols":{
                                "common":{"request":{"header":{"readTimeout":"10s"}}},
                                "aws.protocols#restJson1":{"request":{"header":{"readTimeout":$value}}}
                            }}""",
                        )
                    }
                error.message shouldContain "customizationConfig.protocols.aws.protocols#restJson1.request.header.readTimeout"
                error.message shouldContain "Use `customizationConfig.protocols.common.request.header.readTimeout` instead."
            }
        }
    }

    @Test
    fun `request body poll budget defaults to 32 and zero becomes null`() {
        schemaSettings("").requestBodyReadPollBudget() shouldBe 32
        for (budget in listOf(0, 1, 7, Int.MAX_VALUE)) {
            schemaSettings("", bodySettings("""{"readPollBudget":$budget}"""))
                .requestBodyReadPollBudget() shouldBe budget.takeIf { it > 0 }
        }
    }

    @Test
    fun `invalid common request settings fail in both serde modes`() {
        for (schemaSerde in listOf(false, true)) {
            val codegen = "\"schemaSerde\":$schemaSerde"
            for (value in listOf("-1", "1.5", "2147483648", "9223372036854775808", "true", "null", "\"32\"", "{}", "[]")) {
                val error =
                    shouldThrow<CodegenException> {
                        settings(codegen, bodySettings("""{"readPollBudget":$value}"""))
                    }
                error.message shouldContain "customizationConfig.protocols.common.request.body.readPollBudget"
            }
            for (value in listOf("-1", "1.5", "9223372036854775808", "true", "null", "\"8\"", "\"1.5 MB\"", "\"2 PB\"")) {
                val error =
                    shouldThrow<CodegenException> {
                        settings(codegen, bodySettings("""{"maxBytes":$value}"""))
                    }
                error.message shouldContain "customizationConfig.protocols.common.request.body.maxBytes"
            }
            for (customization in listOf(
                """{"protocols":7}""",
                """{"protocols":{"common":null}}""",
                """{"protocols":{"common":{"request":[]}}}""",
                bodySettings("false"),
            )) {
                shouldThrow<CodegenException> { settings(codegen, customization) }
            }
        }
    }

    @Test
    fun `legacy limit folds into nested settings and preserves other customization`() {
        for (schemaSerde in listOf(false, true)) {
            val loaded =
                settings(
                    "\"schemaSerde\":$schemaSerde,\"requestBodyMaxBytes\":1024",
                    """{"example":true,"protocols":{"example#protocol":{"enabled":true}}}""",
                )
            loaded.requestBodyMaxBytes() shouldBe 1024L
            loaded.customizationConfig shouldBe
                Node.parse(
                    """{"example":true,"protocols":{"example#protocol":{"enabled":true},"common":{"request":{"body":{"maxBytes":1024}}}}}""",
                ).expectObjectNode()
        }
    }

    @Test
    fun `nested body limit takes precedence over legacy setting including zero`() {
        for (schemaSerde in listOf(false, true)) {
            for (limit in listOf(0, 2048)) {
                val customization = bodySettings("""{"maxBytes":$limit}""")
                val loaded = settings("\"schemaSerde\":$schemaSerde,\"requestBodyMaxBytes\":1024", customization)
                loaded.requestBodyMaxBytes() shouldBe limit.toLong()
                loaded.customizationConfig shouldBe Node.parse(customization).expectObjectNode()
            }
            shouldThrow<CodegenException> {
                settings("\"schemaSerde\":$schemaSerde,\"requestBodyMaxBytes\":1024", bodySettings("""{"maxBytes":null}"""))
            }
        }
    }

    @Test
    fun `invalid legacy body limits fail in both serde modes`() {
        for (schemaSerde in listOf(false, true)) {
            for (value in listOf("-1", "1.5", "9223372036854775808", "true", "null", "\"8\"")) {
                shouldThrow<Exception> { settings("\"schemaSerde\":$schemaSerde,\"requestBodyMaxBytes\":$value") }
            }
        }
    }

    @Test
    fun `legacy limit emits one deprecation warning and nested conflicts explain precedence`() {
        val logger = Logger.getLogger("ServerRustSettings")
        val warnings = mutableListOf<String>()
        val testThread = Thread.currentThread()
        val handler =
            object : Handler() {
                override fun publish(record: LogRecord) {
                    if (Thread.currentThread() == testThread && record.level == Level.WARNING) {
                        warnings.add(record.message)
                    }
                }

                override fun flush() = Unit

                override fun close() = Unit
            }
        logger.addHandler(handler)
        try {
            for (schemaSerde in listOf(false, true)) {
                val codegen = "\"schemaSerde\":$schemaSerde"
                for (legacy in listOf(0L, 512L)) {
                    warnings.clear()
                    val loaded = settings("$codegen,\"requestBodyMaxBytes\":$legacy")
                    loaded.requestBodyMaxBytes() shouldBe legacy
                    loaded.protocolSettings()
                    loaded.requestBodyMaxBytes()
                    warnings.single() shouldContain "`codegen.requestBodyMaxBytes` is deprecated"
                    warnings.single() shouldContain "customizationConfig.protocols.common.request.body.maxBytes"
                }
                warnings.clear()
                settings("$codegen,\"requestBodyMaxBytes\":512", bodySettings("""{"maxBytes":0}"""))
                    .requestBodyMaxBytes() shouldBe 0L
                warnings.count { it.contains("deprecated") } shouldBe 1
                warnings.any { it.contains("using the nested value") } shouldBe true
                warnings.clear()
                settings(codegen, bodySettings("""{"maxBytes":0}"""))
                settings(codegen)
                warnings shouldBe emptyList()
                settings(codegen, """{"protocols":{"common":{"requestBodyMaxBytes":128}}}""")
                    .requestBodyMaxBytes() shouldBe null
                warnings.single() shouldContain "Unknown setting(s)"
            }
        } finally {
            logger.removeHandler(handler)
            handler.close()
        }
    }

    @Test
    fun `protocol settings pass through arbitrary sections keyed by protocol shape ID`() {
        val sections =
            schemaSettings(
                "",
                """{ "protocols": { "example.protocols#customRpc": { "anything": { "nested": 1 } } } }""",
            ).protocolSettings()
        sections.keys shouldBe setOf("example.protocols#customRpc")
        sections.getValue("example.protocols#customRpc") shouldBe
            Node.parse("""{ "anything": { "nested": 1 } }""").expectObjectNode()
    }

    @Test
    fun `full IDs for built in and custom protocols are preserved in both modes`() {
        val ids =
            listOf(
                "aws.protocols#awsJson1_0",
                "aws.protocols#awsJson1_1",
                "aws.protocols#restJson1",
                "aws.protocols#restXml",
                ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID,
                "example.protocols#customRpc",
                ServerRustSettings.COMMON_PROTOCOL_SETTINGS_KEY,
            )
        val sections = ids.joinToString(",") { """"$it": {"extension": true}""" }
        for (schemaSerde in listOf(false, true)) {
            val parsed =
                settings(
                    """"schemaSerde": $schemaSerde""",
                    """{"protocols": {$sections}}""",
                ).protocolSettings()
            parsed.keys shouldBe ids.toSet()
            parsed.values.forEach {
                it shouldBe Node.parse("""{"extension": true}""").expectObjectNode()
            }
        }
    }

    @Test
    fun `protocol shorthand and invalid shape IDs are rejected in both modes`() {
        val invalidIds =
            listOf(
                "awsJson1_0",
                "awsJson1_1",
                "restJson1",
                "restXml",
                "rpcv2Cbor",
                "rpcv1",
                "aws.protocols#",
                "not a shape ID",
                "aws.protocols#restJson1${'$'}member",
            )
        for (schemaSerde in listOf(false, true)) {
            for (id in invalidIds) {
                val error =
                    shouldThrow<CodegenException> {
                        settings(
                            """"schemaSerde": $schemaSerde""",
                            """{"protocols": {"$id": {}}}""",
                        )
                    }
                error.message shouldContain "`customizationConfig.protocols.$id`"
                error.message shouldContain "full protocol shape ID"
            }
        }
    }

    @Test
    fun `capitalizeRoutes is read from customizationConfig protocols`() {
        val settings =
            schemaSettings(
                "",
                """{ "protocols": { "${ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID}": { "capitalizeRoutes": true } } }""",
            )
        settings.rpcV2CborCapitalizeRoutes() shouldBe true
    }

    @Test
    fun `legacy rpcV2CborAddCapitalizedRoute folds into the protocol section`() {
        val settings = schemaSettings(""""${ServerCodegenConfig.RPC_V2_CBOR_ADD_CAPITALIZED_ROUTE_CONFIG_KEY}": true""")
        settings.rpcV2CborCapitalizeRoutes() shouldBe true
        settings.protocolSettings().getValue(ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID)
            .expectBooleanMember(ServerRustSettings.CAPITALIZE_ROUTES_KEY).value shouldBe true
    }

    @Test
    fun `customizationConfig wins over the legacy flag on conflict`() {
        val settings =
            schemaSettings(
                """"${ServerCodegenConfig.RPC_V2_CBOR_ADD_CAPITALIZED_ROUTE_CONFIG_KEY}": true""",
                """{ "protocols": { "${ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID}": { "capitalizeRoutes": false } } }""",
            )
        settings.rpcV2CborCapitalizeRoutes() shouldBe false
    }

    @Test
    fun `legacy flag folds into an existing section without other capitalizeRoutes`() {
        val settings =
            schemaSettings(
                """"${ServerCodegenConfig.RPC_V2_CBOR_ADD_CAPITALIZED_ROUTE_CONFIG_KEY}": true""",
                """{ "protocols": { "${ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID}": { "otherSetting": 1 } } }""",
            )
        settings.rpcV2CborCapitalizeRoutes() shouldBe true
        settings.protocolSettings().getValue(ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID)
            .expectNumberMember("otherSetting").value shouldBe 1
    }

    @Test
    fun `generated serde preserves historical response bindings while retaining protocol sections`() {
        val loaded = settings("", """{"protocols":{"aws.protocols#awsJson1_0":{"responseBindings":"smithy-specs"}}}""")
        loaded.protocolSettings().keys shouldBe setOf("aws.protocols#awsJson1_0")
        loaded.protocolResponseSettings.responseBindings(
            software.amazon.smithy.model.shapes.ShapeId.from("aws.protocols#awsJson1_0"),
        ) shouldBe AwsJsonResponseBindings.HISTORICAL
    }
}
