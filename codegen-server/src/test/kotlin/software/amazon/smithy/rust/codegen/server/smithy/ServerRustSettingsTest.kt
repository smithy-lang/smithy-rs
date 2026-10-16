/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import io.kotest.assertions.throwables.shouldThrow
import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rust.codegen.core.smithy.HttpVersion
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel

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
                settings.runtimeConfig.httpVersion shouldBe if (http1x) HttpVersion.Http1x else HttpVersion.Http0x
            }
        }
    }

    @Test
    fun `protocol settings are empty by default`() {
        settings("").protocolSettings() shouldBe emptyMap()
        settings("").rpcV2CborCapitalizeRoutes() shouldBe false
    }

    @Test
    fun `requestBodyMaxBytes is read from the global protocols section`() {
        settings("").requestBodyMaxBytes() shouldBe 0L
        settings("", """{ "protocols": { "global": { "otherSetting": 1 } } }""").requestBodyMaxBytes() shouldBe 0L
        settings("", """{ "protocols": { "global": { "requestBodyMaxBytes": 2048 } } }""").requestBodyMaxBytes() shouldBe 2048L
    }

    @Test
    fun `legacy request body limit is copied into customization config`() {
        for (customization in listOf(null, "{}", """{ "protocols": {} }""")) {
            val settings = settings(""""requestBodyMaxBytes": 1024""", customization)
            settings.requestBodyMaxBytes() shouldBe 1024L
            settings.customizationConfig shouldBe
                Node.parse("""{ "protocols": { "global": { "requestBodyMaxBytes": 1024 } } }""").expectObjectNode()
        }
    }

    @Test
    fun `copying the legacy limit preserves other customization settings`() {
        val settings =
            settings(
                """"requestBodyMaxBytes": 1024""",
                """{ "example": true, "protocols": { "global": { "otherSetting": 1 }, "example#protocol": { "enabled": true } } }""",
            )
        settings.requestBodyMaxBytes() shouldBe 1024L
        settings.customizationConfig shouldBe
            Node.parse(
                """{ "example": true, "protocols": { "global": { "otherSetting": 1, "requestBodyMaxBytes": 1024 }, "example#protocol": { "enabled": true } } }""",
            ).expectObjectNode()
    }

    @Test
    fun `explicit global body limit takes precedence over the legacy setting including zero`() {
        for (limit in listOf(0, 2048)) {
            val customization = """{ "protocols": { "global": { "requestBodyMaxBytes": $limit } } }"""
            val settings = settings(""""requestBodyMaxBytes": 1024""", customization)
            settings.requestBodyMaxBytes() shouldBe limit.toLong()
            settings.customizationConfig shouldBe Node.parse(customization).expectObjectNode()
        }
    }

    @Test
    fun `invalid legacy body limits are rejected when copied`() {
        for (value in listOf("-1", "1.5", "9223372036854775808", "true", "null", "\"8\"")) {
            shouldThrow<Exception> {
                settings(""""requestBodyMaxBytes": $value""")
            }
        }
    }

    @Test
    fun `invalid explicit global limit is not replaced by the legacy setting`() {
        shouldThrow<Exception> {
            settings(
                """"requestBodyMaxBytes": 1024""",
                """{ "protocols": { "global": { "requestBodyMaxBytes": null } } }""",
            )
        }
    }

    @Test
    fun `negative requestBodyMaxBytes is rejected when settings are loaded`() {
        shouldThrow<IllegalArgumentException> {
            settings("", """{ "protocols": { "global": { "requestBodyMaxBytes": -1 } } }""")
        }
    }

    @Test
    fun `invalid request body limits are rejected during codegen`() {
        for (value in listOf("1.5", "9223372036854775808", "true", "null", "\"8\"")) {
            shouldThrow<Exception> {
                settings("", """{ "protocols": { "global": { "requestBodyMaxBytes": $value } } }""")
            }
        }
        shouldThrow<Exception> {
            settings("", """{ "protocols": { "global": "not an object" } }""")
        }
    }

    @Test
    fun `protocol settings pass through arbitrary sections keyed by protocol shape ID`() {
        val sections =
            settings(
                "",
                """{ "protocols": { "com.amazon.coral#rpcv1": { "anything": { "nested": 1 } } } }""",
            ).protocolSettings()
        sections.keys shouldBe setOf("com.amazon.coral#rpcv1")
        sections.getValue("com.amazon.coral#rpcv1") shouldBe
            Node.parse("""{ "anything": { "nested": 1 } }""").expectObjectNode()
    }

    @Test
    fun `capitalizeRoutes is read from customizationConfig protocols`() {
        val settings =
            settings(
                "",
                """{ "protocols": { "${ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID}": { "capitalizeRoutes": true } } }""",
            )
        settings.rpcV2CborCapitalizeRoutes() shouldBe true
    }

    @Test
    fun `legacy rpcV2CborAddCapitalizedRoute folds into the protocol section`() {
        val settings = settings(""""${ServerCodegenConfig.RPC_V2_CBOR_ADD_CAPITALIZED_ROUTE_CONFIG_KEY}": true""")
        settings.rpcV2CborCapitalizeRoutes() shouldBe true
        settings.protocolSettings().getValue(ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID)
            .expectBooleanMember(ServerRustSettings.CAPITALIZE_ROUTES_KEY).value shouldBe true
    }

    @Test
    fun `customizationConfig wins over the legacy flag on conflict`() {
        val settings =
            settings(
                """"${ServerCodegenConfig.RPC_V2_CBOR_ADD_CAPITALIZED_ROUTE_CONFIG_KEY}": true""",
                """{ "protocols": { "${ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID}": { "capitalizeRoutes": false } } }""",
            )
        settings.rpcV2CborCapitalizeRoutes() shouldBe false
    }

    @Test
    fun `legacy flag folds into an existing section without other capitalizeRoutes`() {
        val settings =
            settings(
                """"${ServerCodegenConfig.RPC_V2_CBOR_ADD_CAPITALIZED_ROUTE_CONFIG_KEY}": true""",
                """{ "protocols": { "${ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID}": { "otherSetting": 1 } } }""",
            )
        settings.rpcV2CborCapitalizeRoutes() shouldBe true
        settings.protocolSettings().getValue(ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID)
            .expectNumberMember("otherSetting").value shouldBe 1
    }
}
