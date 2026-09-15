/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
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
    fun `protocol settings are empty by default`() {
        settings("").protocolSettings() shouldBe emptyMap()
        settings("").rpcV2CborCapitalizeRoutes() shouldBe false
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
