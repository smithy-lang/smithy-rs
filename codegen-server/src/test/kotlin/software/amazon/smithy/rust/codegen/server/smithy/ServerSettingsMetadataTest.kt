/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import com.moandjiezana.toml.Toml
import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.generators.CargoTomlGenerator
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

internal class ServerSettingsMetadataTest {
    private val model =
        """
        ${'$'}version: "2"
        namespace test
        use aws.protocols#restJson1
        @restJson1
        service TestService { version: "1", operations: [Ping] }
        @http(method: "GET", uri: "/ping")
        operation Ping {}
    """.asSmithyModel()

    private fun metadata(config: String = ""): Toml {
        val settings =
            ServerRustSettings.from(
                model,
                Node.parse(
                    """{
                "module": "test-service", "moduleVersion": "1.0.0", "moduleAuthors": ["test"],
                "service": "test#TestService" $config
            }""",
                ).expectObjectNode(),
            )
        val protocols = listOf("aws.protocols#restXml", ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID).map(ShapeId::from)
        val writer = RustWriter.toml("Cargo.toml")
        CargoTomlGenerator(settings, protocols.first().toString(), writer, settings.manifestSettingsMetadata(protocols), emptyList(), emptyList()).render()
        return Toml().read(writer.toString()).getTable("package.metadata")
    }

    // toml4j retains the quotation marks on quoted table names in toMap().
    private fun Toml.protocolTables(): Map<String, Any> =
        getTable("customizationConfig.protocols").toMap().mapKeys { (key, _) -> key.removeSurrounding("\"") }

    @Test
    fun `manifest records default server flags and built-in protocol defaults`() {
        val metadata = metadata()
        val codegen = metadata.getTable("codegen")
        codegen.getBoolean("debugMode") shouldBe false
        codegen.getLong("formatTimeoutSeconds") shouldBe 20L
        codegen.getBoolean("publicConstrainedTypes") shouldBe true
        codegen.getBoolean("schemaSerde") shouldBe false
        codegen.getBoolean("http-1x") shouldBe false
        codegen.getLong("requestBodyMaxBytes") shouldBe 0L
        // Unset means automatic; it is not an explicit false override.
        codegen.contains("addValidationExceptionToConstrainedOperations") shouldBe false
        val protocols = metadata.protocolTables()
        protocols["aws.protocols#restXml"] shouldBe mapOf("legacyMode" to false)
        protocols[ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID] shouldBe mapOf("capitalizeRoutes" to false)
    }

    @Test
    fun `configured flags override defaults and nested customization values keep their types`() {
        val metadata =
            metadata(
                """,
            "codegen": {"schemaSerde": true, "http-1x": true, "requestBodyMaxBytes": 4096,
                        "addValidationExceptionToConstrainedOperations": false, "rpcV2CborAddCapitalizedRoute": true},
            "customizationConfig": {"protocols": {
                "smithy.protocols#rpcv2Cbor": {"capitalizeRoutes": false},
                "aws.protocols#restXml": {"legacyMode": true},
                "example#custom": {"nested": {"enabled": true, "limit": 3, "names": ["one", "two"]}}
            }}
        """,
            )
        val codegen = metadata.getTable("codegen")
        codegen.getBoolean("schemaSerde") shouldBe true
        codegen.getBoolean("http-1x") shouldBe true
        codegen.getLong("requestBodyMaxBytes") shouldBe 4096L
        codegen.getBoolean("addValidationExceptionToConstrainedOperations") shouldBe false
        codegen.getBoolean("debugMode") shouldBe false
        val protocols = metadata.protocolTables()
        protocols[ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID] shouldBe mapOf("capitalizeRoutes" to false)
        protocols["aws.protocols#restXml"] shouldBe mapOf("legacyMode" to true)
        protocols["example#custom"] shouldBe mapOf("nested" to mapOf("enabled" to true, "limit" to 3L, "names" to listOf("one", "two")))
    }

    @Test
    fun `metadata records the legacy route flag folded into customization settings`() {
        val metadata = metadata(""", "codegen": {"rpcV2CborAddCapitalizedRoute": true}""")
        metadata.protocolTables()[ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID] shouldBe mapOf("capitalizeRoutes" to true)
    }

    @Test
    fun `server plugin includes effective settings in the generated manifest`() {
        val server =
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings =
                        Node.parse(
                            """{
                "codegen": {"requestBodyMaxBytes": 1024},
                "customizationConfig": {"example": {"enabled": true}}
            }""",
                        ).expectObjectNode(),
                ),
                testCoverage = HttpTestType.Default,
            ).single()
        val metadata = Toml().read(server.path.resolve("Cargo.toml").toFile()).getTable("package.metadata")
        metadata.getLong("codegen.requestBodyMaxBytes") shouldBe 1024L
        metadata.getBoolean("codegen.schemaSerde") shouldBe false
        metadata.getBoolean("customizationConfig.example.enabled") shouldBe true
    }
}
